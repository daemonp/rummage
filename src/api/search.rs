use crate::db::DbHandle;
use crate::error::{AppError, Result};
use axum::extract::{Query, State};
use axum::Json;
use notmuch::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub sort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadList {
    pub query: String,
    pub total_count: usize,
    pub threads: Vec<ThreadSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub thread_id: String,
    pub subject: String,
    pub authors: String,
    pub matched_messages: i32,
    pub total_messages: i32,
    pub newest_date: i64,
    pub oldest_date: i64,
    pub tags: Vec<String>,
    pub preview: Option<String>,
    pub has_attachments: bool,
}

/// Search email threads via notmuch query syntax.
///
/// # Errors
/// Returns `AppError::Notmuch` on database failures, or `AppError::Internal`
/// if the DB worker channel closes unexpectedly.
pub async fn handler(
    State(db): State<DbHandle>,
    Query(params): Query<SearchParams>,
) -> Result<Json<ThreadList>> {
    let result = db
        .search(params.q, params.offset, params.limit, params.sort)
        .await?;
    Ok(Json(result))
}

/// Synchronous search against an open notmuch `Database`.
///
/// # Errors
/// Returns `AppError::Notmuch` on query or iteration failures.
pub fn do_search(
    db: &Database,
    q: &str,
    offset: Option<usize>,
    limit: Option<usize>,
    sort: Option<&str>,
) -> Result<ThreadList> {
    let query = db.create_query(q).map_err(AppError::Notmuch)?;

    // Apply sort.
    match sort {
        Some("oldest") => query.set_sort(notmuch::Sort::OldestFirst),
        _ => query.set_sort(notmuch::Sort::NewestFirst),
    }

    let total_count = query.count_threads().map_err(AppError::Notmuch)? as usize;

    let threads = query.search_threads().map_err(AppError::Notmuch)?;

    let limit = limit.map(|l| l.min(100)).unwrap_or(20);
    let offset = offset.unwrap_or(0);

    let summaries: Vec<ThreadSummary> = threads
        .skip(offset)
        .take(limit)
        .map(|thread| {
            let tags: Vec<String> = thread.tags().collect();
            let has_attachments = tags.iter().any(|t| t.contains("attachment"));

            // Build preview from the first matched message in the thread.
            let preview = thread.messages().next().and_then(|msg| {
                crate::mail::extract_message(&msg)
                    .ok()
                    .and_then(|detail| {
                        detail.body_text.or_else(|| {
                            if detail.content_type == "text/plain" {
                                Some(detail.content)
                            } else {
                                None
                            }
                        })
                    })
                    .map(|text| crate::mail::body::truncate_text(&text, 200))
            });

            ThreadSummary {
                thread_id: thread.id().to_string(),
                subject: thread.subject().to_string(),
                authors: thread.authors().join(", "),
                matched_messages: thread.matched_messages(),
                total_messages: thread.total_messages(),
                newest_date: thread.newest_date(),
                oldest_date: thread.oldest_date(),
                tags,
                preview,
                has_attachments,
            }
        })
        .collect();

    Ok(ThreadList {
        query: q.to_string(),
        total_count,
        threads: summaries,
    })
}
