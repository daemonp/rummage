use crate::db::DbHandle;
use crate::error::{AppError, Result};
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DateRange {
    pub oldest: String,
    pub newest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveStats {
    pub total_messages: usize,
    pub total_threads: usize,
    pub tag_count: usize,
    pub date_range: Option<DateRange>,
}

pub async fn handler(State(db): State<DbHandle>) -> Result<Json<ArchiveStats>> {
    let stats = db.stats().await?;
    Ok(Json(stats))
}

/// Synchronous archive stats against an open notmuch `Database`.
///
/// # Errors
/// Returns `AppError::Notmuch` on query failures.
pub fn do_stats(db: &notmuch::Database) -> Result<ArchiveStats> {
    let query = db.create_query("*").map_err(AppError::Notmuch)?;
    let total_messages = query.count_messages().map_err(AppError::Notmuch)? as usize;
    let total_threads = query.count_threads().map_err(AppError::Notmuch)? as usize;

    let tag_count = db.all_tags().map_err(AppError::Notmuch)?.count();

    let date_range = {
        let mut oldest = None;
        let mut newest = None;

        // Oldest message
        if let Ok(q) = db.create_query("*") {
            q.set_sort(notmuch::Sort::OldestFirst);
            if let Ok(msgs) = q.search_messages() {
                if let Some(msg) = msgs.into_iter().next() {
                    oldest = chrono::DateTime::from_timestamp(msg.date(), 0)
                        .map(|dt| dt.format("%Y-%m-%d").to_string());
                }
            }
        }

        // Newest message
        if let Ok(q) = db.create_query("*") {
            q.set_sort(notmuch::Sort::NewestFirst);
            if let Ok(msgs) = q.search_messages() {
                if let Some(msg) = msgs.into_iter().next() {
                    newest = chrono::DateTime::from_timestamp(msg.date(), 0)
                        .map(|dt| dt.format("%Y-%m-%d").to_string());
                }
            }
        }

        oldest.and_then(|o| {
            newest.map(|n| DateRange {
                oldest: o,
                newest: n,
            })
        })
    };

    Ok(ArchiveStats {
        total_messages,
        total_threads,
        tag_count,
        date_range,
    })
}

/// Count threads and messages matching a notmuch query.
///
/// # Errors
/// Returns `AppError::Notmuch` on query failures.
pub fn do_count(db: &notmuch::Database, q: &str) -> Result<(usize, usize)> {
    let query = db.create_query(q).map_err(AppError::Notmuch)?;
    let thread_count = query.count_threads().map_err(AppError::Notmuch)? as usize;
    let message_count = query.count_messages().map_err(AppError::Notmuch)? as usize;
    Ok((thread_count, message_count))
}
