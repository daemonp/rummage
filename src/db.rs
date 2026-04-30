//! Single-threaded notmuch database worker.
//!
//! All `libnotmuch` access is serialized through a single `spawn_blocking`
//! thread.  Callers interact via the [`DbHandle`] which sends requests over
//! an `mpsc` channel and awaits responses on a `oneshot`.

use crate::api::search::ThreadList;
use crate::api::stats::ArchiveStats;
use crate::api::thread::{ConversationTree, ThreadDetail};
use crate::error::{AppError, Result};
use std::path::Path;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{error, info, warn};

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

/// Lifecycle state of the DB worker, exposed via a `watch` channel.
#[derive(Clone, Debug)]
pub enum DbState {
    Initializing,
    Ready,
    Failed(String),
}

/// Handle to the DB worker. Clone freely — it is just an `mpsc::Sender` wrapper.
#[derive(Clone)]
pub struct DbHandle {
    sender: mpsc::Sender<DbRequest>,
    state: watch::Receiver<DbState>,
}

impl DbHandle {
    /// Send a request to the DB worker and await the response.
    ///
    /// This is the single entry-point for all DB communication, eliminating
    /// the send/receive boilerplate from each public method.
    async fn request<T>(
        &self,
        make_req: impl FnOnce(oneshot::Sender<Result<T>>) -> DbRequest,
    ) -> Result<T> {
        match self.state.borrow().clone() {
            DbState::Ready => {}
            DbState::Failed(ref e) => {
                return Err(AppError::ServiceUnavailable(e.clone()));
            }
            DbState::Initializing => {
                return Err(AppError::ServiceUnavailable(
                    "Database is still initializing".into(),
                ));
            }
        }
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(make_req(tx))
            .await
            .map_err(|_| AppError::Internal("DB worker channel closed".into()))?;
        rx.await
            .map_err(|_| AppError::Internal("DB worker dropped response".into()))?
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

    /// List all tags with their message counts.
    pub async fn tags(&self) -> Result<Vec<(String, usize)>> {
        self.request(|tx| DbRequest::Tags { respond: tx }).await
    }

    /// Get archive-wide statistics.
    pub async fn stats(&self) -> Result<ArchiveStats> {
        self.request(|tx| DbRequest::Stats { respond: tx }).await
    }

    /// Get top senders with message counts.
    pub async fn senders(&self) -> Result<Vec<(String, usize)>> {
        self.request(|tx| DbRequest::Senders { respond: tx }).await
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
            match rx.borrow().clone() {
                DbState::Ready => return Ok(()),
                DbState::Failed(ref e) => return Err(AppError::ServiceUnavailable(e.clone())),
                DbState::Initializing => {}
            }
            if rx.changed().await.is_err() {
                return Err(AppError::Internal("DB worker dropped state channel".into()));
            }
        }
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
    std::fs::read(filename).map_err(AppError::Io)
}

// ── Worker lifecycle ───────────────────────────────────────────────

/// Spawn the notmuch DB worker thread and return a handle to it.
///
/// # Errors
/// Returns an error if the database does not exist and cannot be created,
/// or if the notmuch worker thread panics or fails to start.
pub async fn spawn_database_worker(
    maildir: &Path,
    config_path: Option<&Path>,
    no_auto_index: bool,
) -> Result<DbHandle> {
    let maildir = maildir.to_owned();
    let config_path = config_path.map(std::borrow::ToOwned::to_owned);

    let (startup_tx, startup_rx) = oneshot::channel::<Result<()>>();
    let (sender, mut receiver) = mpsc::channel::<DbRequest>(32);
    let (state_tx, state_rx) = watch::channel(DbState::Initializing);

    tokio::task::spawn_blocking(move || {
        let db_result = (|| -> Result<notmuch::Database> {
            let db_path = maildir.join(".notmuch");

            if !db_path.exists() {
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

            let db = notmuch::Database::open_with_config(
                Some(&maildir),
                notmuch::DatabaseMode::ReadOnly,
                config_path.as_deref(),
                None,
            )?;

            info!("Database ready");
            Ok(db)
        })();

        match db_result {
            Ok(db) => {
                let _ = state_tx.send(DbState::Ready);
                if startup_tx.send(Ok(())).is_err() {
                    return;
                }
                while let Some(req) = receiver.blocking_recv() {
                    let _span = match &req {
                        DbRequest::Search { .. } => {
                            tracing::info_span!("db_request", kind = "search")
                        }
                        DbRequest::Thread { .. } => {
                            tracing::info_span!("db_request", kind = "thread")
                        }
                        DbRequest::Attachment { .. } => {
                            tracing::info_span!("db_request", kind = "attachment")
                        }
                        DbRequest::RawMessage { .. } => {
                            tracing::info_span!("db_request", kind = "raw_message")
                        }
                        DbRequest::Tags { .. } => tracing::info_span!("db_request", kind = "tags"),
                        DbRequest::Stats { .. } => {
                            tracing::info_span!("db_request", kind = "stats")
                        }
                        DbRequest::Senders { .. } => {
                            tracing::info_span!("db_request", kind = "senders")
                        }
                        DbRequest::Count { .. } => {
                            tracing::info_span!("db_request", kind = "count")
                        }
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
                                &db,
                                &query,
                                offset,
                                limit,
                                sort.as_deref(),
                            ));
                        }
                        DbRequest::Thread { thread_id, respond } => {
                            let _ = respond.send(crate::api::thread::do_thread(&db, &thread_id));
                        }
                        DbRequest::Attachment {
                            msg_id,
                            part_num,
                            respond,
                        } => {
                            let _ = respond.send(crate::api::attachment::do_attachment(
                                &db, &msg_id, part_num,
                            ));
                        }
                        DbRequest::RawMessage { msg_id, respond } => {
                            let _ = respond.send(crate::api::message::do_raw_message(&db, &msg_id));
                        }
                        DbRequest::Tags { respond } => {
                            let _ = respond.send(crate::api::tags::do_tags(&db));
                        }
                        DbRequest::Stats { respond } => {
                            let _ = respond.send(crate::api::stats::do_stats(&db));
                        }
                        DbRequest::Senders { respond } => {
                            let _ = respond.send(crate::api::senders::do_senders(&db));
                        }
                        DbRequest::Count { query, respond } => {
                            let _ = respond.send(crate::api::stats::do_count(&db, &query));
                        }
                        DbRequest::MessageDetail { msg_id, respond } => {
                            let _ =
                                respond.send(crate::api::message::do_message_detail(&db, &msg_id));
                        }
                        DbRequest::SendersWithQuery {
                            query,
                            limit,
                            respond,
                        } => {
                            let _ = respond.send(crate::api::senders::do_senders_with_query(
                                &db,
                                query.as_deref(),
                                limit,
                            ));
                        }
                        DbRequest::ThreadTree { thread_id, respond } => {
                            let _ =
                                respond.send(crate::api::thread::do_thread_tree(&db, &thread_id));
                        }
                        DbRequest::AttachmentText {
                            msg_id,
                            part,
                            format,
                            respond,
                        } => {
                            let _ = respond.send(crate::api::attachment::do_attachment_text(
                                &db, &msg_id, part, &format,
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                let _ = state_tx.send(DbState::Failed(e.to_string()));
                let _ = startup_tx.send(Err(e));
            }
        }
    });

    // Don't block server startup on DB initialization.  Await the startup
    // confirmation in a background task so the HTTP server can bind immediately
    // and serve the /api/health endpoint (and return 503 for DB endpoints
    // while indexing is still underway).
    tokio::spawn(async move {
        match startup_rx.await {
            Ok(Ok(())) => info!("Database worker initialized successfully"),
            Ok(Err(e)) => error!("Database worker initialization failed: {}", e),
            Err(_) => error!("Database worker dropped startup signal"),
        }
    });

    Ok(DbHandle {
        sender,
        state: state_rx,
    })
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
                        let elapsed = start.elapsed().as_secs_f64();
                        let rate = if elapsed > 0.0 {
                            indexed as f64 / elapsed
                        } else {
                            0.0
                        };
                        info!("Indexed {} messages ({:.0} msg/s)...", indexed, rate);
                        last_reported = indexed;
                    }
                }
            }
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let rate = if elapsed > 0.0 {
        indexed as f64 / elapsed
    } else {
        0.0
    };
    info!(
        "Indexed {} messages in {:.1}s ({:.0} msg/s)",
        indexed, elapsed, rate
    );
}
