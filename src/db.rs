//! Multi-threaded notmuch database access.
//!
//! Each DB request is dispatched to a fresh `tokio::task::spawn_blocking`
//! task that opens its own read-only `notmuch::Database` handle.
//! This mirrors the netviel approach and eliminates the single-threaded
//! bottleneck of the previous serialized worker.

use crate::api::search::ThreadList;
use crate::api::stats::ArchiveStats;
use crate::api::thread::{ConversationTree, ThreadDetail};
use crate::error::{AppError, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, watch, RwLock};
use tracing::{error, info, warn};

/// TTL for cached sidebar data (tags, senders, stats).
const SIDEBAR_CACHE_TTL: Duration = Duration::from_secs(30);

/// Attachment data returned from the DB worker.
pub struct AttachmentData {
    pub content_type: String,
    pub body: Vec<u8>,
    pub filename: Option<String>,
}

/// Requests sent to the DB worker task.
pub enum DbRequest {
    Search {
        query: String,
        offset: Option<usize>,
        limit: Option<usize>,
        sort: Option<String>,
        respond: oneshot::Sender<Result<ThreadList>>,
    },
    Thread {
        thread_id: String,
        respond: oneshot::Sender<Result<ThreadDetail>>,
    },
    Attachment {
        msg_id: String,
        part_num: usize,
        respond: oneshot::Sender<Result<AttachmentData>>,
    },
    RawMessage {
        msg_id: String,
        respond: oneshot::Sender<Result<Vec<u8>>>,
    },
    Tags {
        respond: oneshot::Sender<Result<Vec<(String, usize)>>>,
    },
    Stats {
        respond: oneshot::Sender<Result<ArchiveStats>>,
    },
    Senders {
        respond: oneshot::Sender<Result<Vec<(String, usize)>>>,
    },
    Count {
        query: String,
        respond: oneshot::Sender<Result<(usize, usize)>>,
    },
    MessageDetail {
        msg_id: String,
        respond: oneshot::Sender<Result<ThreadDetail>>, // Re-use ThreadDetail for single message
    },
    SendersWithQuery {
        query: Option<String>,
        limit: usize,
        respond: oneshot::Sender<Result<Vec<(String, usize)>>>,
    },
    ThreadTree {
        thread_id: String,
        respond: oneshot::Sender<Result<ConversationTree>>,
    },
    AttachmentText {
        msg_id: String,
        part: usize,
        format: String,
        respond: oneshot::Sender<Result<String>>,
    },
}

impl DbRequest {
    /// Return a static label for the request variant, used as a metric dimension.
    pub fn kind(&self) -> &'static str {
        match self {
            DbRequest::Search { .. } => "search",
            DbRequest::Thread { .. } => "thread",
            DbRequest::Attachment { .. } => "attachment",
            DbRequest::RawMessage { .. } => "raw_message",
            DbRequest::Tags { .. } => "tags",
            DbRequest::Stats { .. } => "stats",
            DbRequest::Senders { .. } => "senders",
            DbRequest::Count { .. } => "count",
            DbRequest::MessageDetail { .. } => "message_detail",
            DbRequest::SendersWithQuery { .. } => "senders_with_query",
            DbRequest::ThreadTree { .. } => "thread_tree",
            DbRequest::AttachmentText { .. } => "attachment_text",
        }
    }
}

/// Lifecycle state of the DB worker, exposed via a `watch` channel.
#[derive(Clone, Debug)]
pub enum DbState {
    Initializing,
    Ready,
    Failed(String),
}

/// Cached sidebar data with timestamps.
#[derive(Default)]
struct Cache {
    tags: Option<(Vec<(String, usize)>, Instant)>,
    senders: Option<(Vec<(String, usize)>, Instant)>,
    stats: Option<(ArchiveStats, Instant)>,
}

/// Handle to the DB layer. Clone freely.
#[derive(Clone)]
pub struct DbHandle {
    maildir: PathBuf,
    config_path: Option<PathBuf>,
    state: watch::Receiver<DbState>,
    cache: Arc<RwLock<Cache>>,
}

impl DbHandle {
    /// Send a request to a fresh blocking task and await the response.
    async fn request<T>(
        &self,
        make_req: impl FnOnce(oneshot::Sender<Result<T>>) -> DbRequest,
    ) -> Result<T> {
        match &*self.state.borrow() {
            DbState::Ready => {}
            DbState::Failed(e) => {
                return Err(AppError::ServiceUnavailable(e.clone()));
            }
            DbState::Initializing => {
                return Err(AppError::ServiceUnavailable(
                    "Database is still initializing".into(),
                ));
            }
        }
        let (tx, rx) = oneshot::channel();
        let req = make_req(tx);
        let kind = req.kind();
        let maildir = self.maildir.clone();
        let config_path = self.config_path.clone();

        tokio::task::spawn_blocking(move || {
            let start = Instant::now();
            let db_result = notmuch::Database::open_with_config(
                Some(&maildir),
                notmuch::DatabaseMode::ReadOnly,
                config_path.as_deref(),
                None,
            )
            .map_err(AppError::Notmuch);
            let success = match db_result {
                Ok(ref db) => {
                    dispatch(db, req);
                    true
                }
                Err(e) => {
                    dispatch_err(req, e);
                    false
                }
            };
            let duration = start.elapsed().as_secs_f64();
            metrics::histogram!("db_request_duration_seconds", "kind" => kind).record(duration);
            metrics::counter!(
                "db_requests_total",
                "kind" => kind,
                "status" => if success { "ok" } else { "error" },
            )
            .increment(1);
        })
        .await
        .map_err(|_| AppError::Internal("DB task panicked".into()))?;

        rx.await
            .map_err(|_| AppError::Internal("DB task dropped response".into()))?
    }

    /// Search email threads by notmuch query.
    pub async fn search(
        &self,
        query: String,
        offset: Option<usize>,
        limit: Option<usize>,
        sort: Option<String>,
    ) -> Result<ThreadList> {
        self.request(|tx| DbRequest::Search {
            query,
            offset,
            limit,
            sort,
            respond: tx,
        })
        .await
    }

    /// Fetch a full thread by ID.
    pub async fn thread(&self, thread_id: String) -> Result<ThreadDetail> {
        self.request(|tx| DbRequest::Thread {
            thread_id,
            respond: tx,
        })
        .await
    }

    /// Extract an attachment by message ID and MIME part number.
    pub async fn attachment(&self, msg_id: String, part_num: usize) -> Result<AttachmentData> {
        self.request(|tx| DbRequest::Attachment {
            msg_id,
            part_num,
            respond: tx,
        })
        .await
    }

    /// Download raw `.eml` bytes for a message.
    pub async fn raw_message(&self, msg_id: String) -> Result<Vec<u8>> {
        self.request(|tx| DbRequest::RawMessage {
            msg_id,
            respond: tx,
        })
        .await
    }

    /// List all tags with their message counts (cached).
    pub async fn tags(&self) -> Result<Vec<(String, usize)>> {
        {
            let cache = self.cache.read().await;
            if let Some((ref data, ts)) = cache.tags {
                if ts.elapsed() < SIDEBAR_CACHE_TTL {
                    metrics::counter!("db_cache_hits_total", "kind" => "tags").increment(1);
                    return Ok(data.clone());
                }
            }
        }
        metrics::counter!("db_cache_misses_total", "kind" => "tags").increment(1);
        let result = self.request(|tx| DbRequest::Tags { respond: tx }).await?;
        {
            let mut cache = self.cache.write().await;
            cache.tags = Some((result.clone(), Instant::now()));
        }
        Ok(result)
    }

    /// Get archive-wide statistics (cached).
    pub async fn stats(&self) -> Result<ArchiveStats> {
        {
            let cache = self.cache.read().await;
            if let Some((ref data, ts)) = cache.stats {
                if ts.elapsed() < SIDEBAR_CACHE_TTL {
                    metrics::counter!("db_cache_hits_total", "kind" => "stats").increment(1);
                    return Ok(data.clone());
                }
            }
        }
        metrics::counter!("db_cache_misses_total", "kind" => "stats").increment(1);
        let result = self.request(|tx| DbRequest::Stats { respond: tx }).await?;
        {
            let mut cache = self.cache.write().await;
            cache.stats = Some((result.clone(), Instant::now()));
        }
        Ok(result)
    }

    /// Get top senders with message counts (cached).
    pub async fn senders(&self) -> Result<Vec<(String, usize)>> {
        {
            let cache = self.cache.read().await;
            if let Some((ref data, ts)) = cache.senders {
                if ts.elapsed() < SIDEBAR_CACHE_TTL {
                    metrics::counter!("db_cache_hits_total", "kind" => "senders").increment(1);
                    return Ok(data.clone());
                }
            }
        }
        metrics::counter!("db_cache_misses_total", "kind" => "senders").increment(1);
        let result = self
            .request(|tx| DbRequest::Senders { respond: tx })
            .await?;
        {
            let mut cache = self.cache.write().await;
            cache.senders = Some((result.clone(), Instant::now()));
        }
        Ok(result)
    }

    /// Count threads and messages matching a query.
    pub async fn count(&self, query: String) -> Result<(usize, usize)> {
        self.request(|tx| DbRequest::Count { query, respond: tx })
            .await
    }

    /// Get parsed detail for a single message.
    pub async fn message_detail(&self, msg_id: String) -> Result<ThreadDetail> {
        self.request(|tx| DbRequest::MessageDetail {
            msg_id,
            respond: tx,
        })
        .await
    }

    /// Get top senders scoped to an optional notmuch query.
    pub async fn senders_with_query(
        &self,
        query: Option<String>,
        limit: usize,
    ) -> Result<Vec<(String, usize)>> {
        self.request(|tx| DbRequest::SendersWithQuery {
            query,
            limit,
            respond: tx,
        })
        .await
    }

    /// Get thread tree structure.
    pub async fn thread_tree(&self, thread_id: String) -> Result<ConversationTree> {
        self.request(|tx| DbRequest::ThreadTree {
            thread_id,
            respond: tx,
        })
        .await
    }

    /// Wait until the database worker has finished initialization.
    ///
    /// Returns `Ok(())` when the DB is ready, or `Err` if initialization
    /// failed (e.g. missing notmuch config, corrupt database, etc.).
    pub async fn wait_for_ready(&self) -> Result<()> {
        let mut rx = self.state.clone();
        loop {
            match &*rx.borrow() {
                DbState::Ready => return Ok(()),
                DbState::Failed(e) => return Err(AppError::ServiceUnavailable(e.clone())),
                DbState::Initializing => {}
            }
            if rx.changed().await.is_err() {
                return Err(AppError::Internal("DB state channel closed".into()));
            }
        }
    }

    /// Returns `true` if the database worker is ready to serve requests.
    pub fn is_ready(&self) -> bool {
        matches!(&*self.state.borrow(), DbState::Ready)
    }

    /// Extract attachment text (stub).
    pub async fn attachment_text(
        &self,
        msg_id: String,
        part: usize,
        format: String,
    ) -> Result<String> {
        self.request(|tx| DbRequest::AttachmentText {
            msg_id,
            part,
            format,
            respond: tx,
        })
        .await
    }
}

// ── Request dispatch ───────────────────────────────────────────────

/// Dispatch a request to the appropriate handler using an open DB handle.
fn dispatch(db: &notmuch::Database, req: DbRequest) {
    let _span = match &req {
        DbRequest::Search { .. } => tracing::info_span!("db_request", kind = "search"),
        DbRequest::Thread { .. } => tracing::info_span!("db_request", kind = "thread"),
        DbRequest::Attachment { .. } => tracing::info_span!("db_request", kind = "attachment"),
        DbRequest::RawMessage { .. } => tracing::info_span!("db_request", kind = "raw_message"),
        DbRequest::Tags { .. } => tracing::info_span!("db_request", kind = "tags"),
        DbRequest::Stats { .. } => tracing::info_span!("db_request", kind = "stats"),
        DbRequest::Senders { .. } => tracing::info_span!("db_request", kind = "senders"),
        DbRequest::Count { .. } => tracing::info_span!("db_request", kind = "count"),
        DbRequest::MessageDetail { .. } => {
            tracing::info_span!("db_request", kind = "message_detail")
        }
        DbRequest::SendersWithQuery { .. } => {
            tracing::info_span!("db_request", kind = "senders_with_query")
        }
        DbRequest::ThreadTree { .. } => {
            tracing::info_span!("db_request", kind = "thread_tree")
        }
        DbRequest::AttachmentText { .. } => {
            tracing::info_span!("db_request", kind = "attachment_text")
        }
    }
    .entered();

    match req {
        DbRequest::Search {
            query,
            offset,
            limit,
            sort,
            respond,
        } => {
            let _ = respond.send(crate::api::search::do_search(
                db,
                &query,
                offset,
                limit,
                sort.as_deref(),
            ));
        }
        DbRequest::Thread { thread_id, respond } => {
            let _ = respond.send(crate::api::thread::do_thread(db, &thread_id));
        }
        DbRequest::Attachment {
            msg_id,
            part_num,
            respond,
        } => {
            let _ = respond.send(crate::api::attachment::do_attachment(db, &msg_id, part_num));
        }
        DbRequest::RawMessage { msg_id, respond } => {
            let _ = respond.send(crate::api::message::do_raw_message(db, &msg_id));
        }
        DbRequest::Tags { respond } => {
            let _ = respond.send(crate::api::tags::do_tags(db));
        }
        DbRequest::Stats { respond } => {
            let _ = respond.send(crate::api::stats::do_stats(db));
        }
        DbRequest::Senders { respond } => {
            let _ = respond.send(crate::api::senders::do_senders(db));
        }
        DbRequest::Count { query, respond } => {
            let _ = respond.send(crate::api::stats::do_count(db, &query));
        }
        DbRequest::MessageDetail { msg_id, respond } => {
            let _ = respond.send(crate::api::message::do_message_detail(db, &msg_id));
        }
        DbRequest::SendersWithQuery {
            query,
            limit,
            respond,
        } => {
            let _ = respond.send(crate::api::senders::do_senders_with_query(
                db,
                query.as_deref(),
                limit,
            ));
        }
        DbRequest::ThreadTree { thread_id, respond } => {
            let _ = respond.send(crate::api::thread::do_thread_tree(db, &thread_id));
        }
        DbRequest::AttachmentText {
            msg_id,
            part,
            format,
            respond,
        } => {
            let _ = respond.send(crate::api::attachment::do_attachment_text(
                db, &msg_id, part, &format,
            ));
        }
    }
}

/// Send a DB-open error back through the oneshot channel embedded in the request.
fn dispatch_err(req: DbRequest, err: AppError) {
    let _span = tracing::info_span!("db_request", kind = "error").entered();
    match req {
        DbRequest::Search { respond, .. } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::Thread { respond, .. } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::Attachment { respond, .. } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::RawMessage { respond, .. } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::Tags { respond } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::Stats { respond } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::Senders { respond } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::Count { respond, .. } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::MessageDetail { respond, .. } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::SendersWithQuery { respond, .. } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::ThreadTree { respond, .. } => {
            let _ = respond.send(Err(err));
        }
        DbRequest::AttachmentText { respond, .. } => {
            let _ = respond.send(Err(err));
        }
    }
}

// ── Public helpers for message lookup ──────────────────────────────

/// Look up a single message by its notmuch ID and read its raw bytes.
///
/// This is the shared implementation used by attachment, raw-message, and
/// any future per-message endpoints, eliminating the duplicated
/// `create_query("id:…") → search_messages → next → fs::read` pattern.
///
/// # Errors
/// Returns `AppError::NotFound` if no message matches,
/// or `AppError::Notmuch` / `AppError::Io` on underlying failures.
pub fn find_message_bytes(db: &notmuch::Database, msg_id: &str) -> Result<Vec<u8>> {
    let query = db
        .create_query(&format!("id:{msg_id}"))
        .map_err(AppError::Notmuch)?;
    let mut msgs = query.search_messages().map_err(AppError::Notmuch)?;
    let msg = msgs
        .next()
        .ok_or_else(|| AppError::NotFound(format!("message not found: {msg_id}")))?;
    let filename = msg.filename();
    std::fs::read(filename).map_err(|e| {
        AppError::Io(std::io::Error::new(
            e.kind(),
            format!("failed to read {}: {e}", filename.display()),
        ))
    })
}

// ── Worker lifecycle ───────────────────────────────────────────────

/// Spawn the notmuch DB initialization and return a handle.
///
/// # Errors
/// Returns an error if the database does not exist and cannot be created.
pub async fn spawn_database_worker(
    maildir: &Path,
    config_path: Option<&Path>,
    no_auto_index: bool,
) -> Result<DbHandle> {
    let maildir = maildir.to_owned();
    let config_path = config_path.map(std::borrow::ToOwned::to_owned);

    let (state_tx, state_rx) = watch::channel(DbState::Initializing);

    let init_maildir = maildir.clone();
    let init_config_path = config_path.clone();

    tokio::spawn(async move {
        let init_result = tokio::task::spawn_blocking({
            let maildir = init_maildir;
            let config_path = init_config_path;
            move || {
                let db_path = maildir.join(".notmuch");

                let db_exists = db_path.exists()
                    && std::fs::read_dir(&db_path)
                        .ok()
                        .and_then(|mut rd| rd.next())
                        .is_some();

                if !db_exists {
                    if no_auto_index {
                        return Err(AppError::NotFound(format!(
                            "No notmuch database found at {} and --no-auto-index is set",
                            db_path.display()
                        )));
                    }
                    info!(
                        "No notmuch database found at {}; creating and indexing...",
                        db_path.display()
                    );
                    create_and_index(&maildir, config_path.as_deref())?;
                } else {
                    info!("Opening notmuch database at {}...", db_path.display());
                }

                let _db = notmuch::Database::open_with_config(
                    Some(&maildir),
                    notmuch::DatabaseMode::ReadOnly,
                    config_path.as_deref(),
                    None,
                )
                .map_err(AppError::Notmuch)?;

                info!("Database ready");
                Ok(())
            }
        })
        .await;

        match init_result {
            Ok(Ok(())) => {
                let _ = state_tx.send(DbState::Ready);
            }
            Ok(Err(e)) => {
                let _ = state_tx.send(DbState::Failed(e.to_string()));
                error!("Database initialization failed: {}", e);
            }
            Err(_) => {
                let _ = state_tx.send(DbState::Failed("Initialization panicked".into()));
                error!("Database initialization panicked");
            }
        }
    });

    Ok(DbHandle {
        maildir,
        config_path,
        state: state_rx,
        cache: Arc::new(RwLock::new(Cache::default())),
    })
}

#[cfg(test)]
impl DbHandle {
    /// Build a mock handle for tests with a specific initial state.
    pub fn mock(initializing: bool) -> (Self, watch::Sender<DbState>) {
        let (state_tx, state_rx) = watch::channel(if initializing {
            DbState::Initializing
        } else {
            DbState::Ready
        });
        (
            DbHandle {
                maildir: PathBuf::new(),
                config_path: None,
                state: state_rx,
                cache: Arc::new(RwLock::new(Cache::default())),
            },
            state_tx,
        )
    }
}

/// Force a full re-index of the maildir, then exit.
///
/// # Errors
/// Returns an error if the database cannot be opened or indexed.
pub fn force_reindex(maildir: &Path, config_path: Option<&Path>) -> Result<()> {
    let db_path = maildir.join(".notmuch");

    if db_path.exists() {
        let db = notmuch::Database::open_with_config(
            Some(maildir),
            notmuch::DatabaseMode::ReadWrite,
            config_path,
            None,
        )?;
        index_maildir(&db, maildir);
    } else {
        create_and_index(maildir, config_path)?;
    }
    Ok(())
}

fn create_and_index(maildir: &Path, config_path: Option<&Path>) -> Result<()> {
    info!("Creating notmuch database at {}...", maildir.display());

    // Database::create does not accept a config path, so we create with the
    // default config and then reopen with the user-supplied one if provided.
    let db = notmuch::Database::create(maildir).map_err(AppError::Notmuch)?;
    drop(db);

    let db = notmuch::Database::open_with_config(
        Some(maildir),
        notmuch::DatabaseMode::ReadWrite,
        config_path,
        None,
    )
    .map_err(AppError::Notmuch)?;

    index_maildir(&db, maildir);
    Ok(())
}

/// Calculate average indexing rate since `start`.
fn indexing_rate(indexed: usize, start: &std::time::Instant) -> f64 {
    let elapsed = start.elapsed().as_secs_f64();
    if elapsed > 0.0 {
        indexed as f64 / elapsed
    } else {
        0.0
    }
}

fn index_maildir(db: &notmuch::Database, maildir: &Path) {
    info!("Walking maildir for indexing...");
    let start = std::time::Instant::now();

    let mut indexed = 0usize;
    let mut last_reported = 0usize;
    const REPORT_INTERVAL: usize = 1_000;

    for entry in walkdir::WalkDir::new(maildir)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        let path = entry.path();
        if path.is_file() {
            let s = path.to_string_lossy();
            if s.contains("/cur/") || s.contains("/new/") {
                if let Err(e) = db.index_file(path, None) {
                    warn!("Failed to index {path:?}: {e}");
                } else {
                    indexed += 1;
                    if indexed - last_reported >= REPORT_INTERVAL {
                        info!(
                            "Indexed {} messages ({:.0} msg/s)...",
                            indexed,
                            indexing_rate(indexed, &start)
                        );
                        last_reported = indexed;
                    }
                }
            }
        }
    }

    info!(
        "Indexed {} messages in {:.1}s ({:.0} msg/s)",
        indexed,
        start.elapsed().as_secs_f64(),
        indexing_rate(indexed, &start)
    );
}

// ── Performance tests ───────────────────────────────────────────────

#[cfg(test)]
mod perf_tests {
    use super::*;

    fn test_maildir() -> Option<(PathBuf, Option<PathBuf>)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let maildir = root.join("mail/test-archive");
        let config = root.join("notmuch-config");
        if maildir.exists() {
            Some((maildir, if config.exists() { Some(config) } else { None }))
        } else {
            None
        }
    }

    #[tokio::test]
    async fn perf_search_inbox() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        let start = Instant::now();
        let result = db
            .search("tag:inbox".into(), None, Some(20), None)
            .await
            .expect("search");
        let elapsed = start.elapsed();
        println!(
            "search tag:inbox -> {} threads in {:?}",
            result.threads.len(),
            elapsed
        );
        assert!(
            elapsed.as_millis() < 500,
            "search took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn perf_search_broad_query() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        let start = Instant::now();
        let result = db
            .search("*".into(), None, Some(20), None)
            .await
            .expect("search");
        let elapsed = start.elapsed();
        println!(
            "search * -> {} threads in {:?}",
            result.threads.len(),
            elapsed
        );
        assert!(
            elapsed.as_millis() < 500,
            "broad search took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn perf_tags_uncached() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        let start = Instant::now();
        let tags = db.tags().await.expect("tags");
        let elapsed = start.elapsed();
        println!("tags (uncached) -> {} tags in {:?}", tags.len(), elapsed);
        assert!(
            elapsed.as_millis() < 2000,
            "tags took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn perf_tags_cached() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        // Warm cache
        let _ = db.tags().await.expect("tags");

        let start = Instant::now();
        let tags = db.tags().await.expect("tags cached");
        let elapsed = start.elapsed();
        println!("tags (cached) -> {} tags in {:?}", tags.len(), elapsed);
        assert!(
            elapsed.as_millis() < 100,
            "cached tags took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn perf_senders_uncached() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        let start = Instant::now();
        let senders = db.senders().await.expect("senders");
        let elapsed = start.elapsed();
        println!(
            "senders (uncached) -> {} senders in {:?}",
            senders.len(),
            elapsed
        );
        assert!(
            elapsed.as_millis() < 2000,
            "senders took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn perf_senders_cached() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        // Warm cache
        let _ = db.senders().await.expect("senders");

        let start = Instant::now();
        let senders = db.senders().await.expect("senders cached");
        let elapsed = start.elapsed();
        println!(
            "senders (cached) -> {} senders in {:?}",
            senders.len(),
            elapsed
        );
        assert!(
            elapsed.as_millis() < 100,
            "cached senders took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn perf_stats_uncached() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        let start = Instant::now();
        let stats = db.stats().await.expect("stats");
        let elapsed = start.elapsed();
        println!(
            "stats (uncached) -> {} msgs / {} threads in {:?}",
            stats.total_messages, stats.total_threads, elapsed
        );
        assert!(
            elapsed.as_millis() < 2000,
            "stats took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn perf_stats_cached() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        // Warm cache
        let _ = db.stats().await.expect("stats");

        let start = Instant::now();
        let stats = db.stats().await.expect("stats cached");
        let elapsed = start.elapsed();
        println!(
            "stats (cached) -> {} msgs / {} threads in {:?}",
            stats.total_messages, stats.total_threads, elapsed
        );
        assert!(
            elapsed.as_millis() < 100,
            "cached stats took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn perf_sidebar_parallel_then_cached() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        // First request: parallel uncached
        let start = Instant::now();
        let (tags, senders, stats) = tokio::join!(db.tags(), db.senders(), db.stats());
        let _ = tags.expect("tags");
        let _ = senders.expect("senders");
        let _ = stats.expect("stats");
        let elapsed = start.elapsed();
        println!("sidebar parallel (uncached) in {:?}", elapsed);
        assert!(
            elapsed.as_millis() < 3000,
            "parallel sidebar took too long: {:?}",
            elapsed
        );

        // Second request: all cached
        let start = Instant::now();
        let (tags, senders, stats) = tokio::join!(db.tags(), db.senders(), db.stats());
        let _ = tags.expect("tags");
        let _ = senders.expect("senders");
        let _ = stats.expect("stats");
        let elapsed = start.elapsed();
        println!("sidebar parallel (cached) in {:?}", elapsed);
        assert!(
            elapsed.as_millis() < 200,
            "cached parallel sidebar took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn perf_search_plus_sidebar() {
        let Some((maildir, config)) = test_maildir() else {
            return;
        };
        let db = spawn_database_worker(&maildir, config.as_deref(), false)
            .await
            .expect("spawn worker");
        db.wait_for_ready().await.expect("db ready");

        // Warm sidebar cache
        let _ = db.tags().await;
        let _ = db.senders().await;
        let _ = db.stats().await;

        let start = Instant::now();
        let (results, _tags, _senders, _stats) = tokio::join!(
            db.search("from:example.org".into(), None, Some(20), None),
            db.tags(),
            db.senders(),
            db.stats(),
        );
        let result = results.expect("search");
        let elapsed = start.elapsed();
        println!(
            "search + sidebar (cached) -> {} threads in {:?}",
            result.threads.len(),
            elapsed
        );
        assert!(
            elapsed.as_millis() < 1000,
            "search + cached sidebar took too long: {:?}",
            elapsed
        );
    }
}
