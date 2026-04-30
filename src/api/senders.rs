use crate::db::DbHandle;
use crate::error::{AppError, Result};
use axum::extract::{Query, State};
use axum::Json;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

/// Compiled email regex — built once, reused across all `do_senders` calls.
static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}")
        .expect("static email regex is valid")
});

#[derive(Debug, Deserialize)]
pub struct SendersParams {
    pub limit: Option<usize>,
    pub query: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SenderItem {
    pub email: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct SenderList {
    pub senders: Vec<SenderItem>,
}

pub async fn handler(
    State(db): State<DbHandle>,
    Query(params): Query<SendersParams>,
) -> Result<Json<SenderList>> {
    let limit = params.limit.unwrap_or(20);
    let senders = db.senders_with_query(params.query, limit).await?;
    Ok(Json(SenderList {
        senders: senders
            .into_iter()
            .map(|(email, count)| SenderItem { email, count })
            .collect(),
    }))
}

/// Synchronous top-senders query (default limit 20, all messages).
///
/// # Errors
/// Returns `AppError::Notmuch` on query failures.
pub fn do_senders(db: &notmuch::Database) -> Result<Vec<(String, usize)>> {
    do_senders_with_query(db, None, 20)
}

/// Synchronous top-senders query with an optional notmuch scope and limit.
///
/// # Errors
/// Returns `AppError::Notmuch` on query failures.
pub fn do_senders_with_query(
    db: &notmuch::Database,
    q: Option<&str>,
    limit: usize,
) -> Result<Vec<(String, usize)>> {
    let query_str = q.unwrap_or("*");
    let query = db.create_query(query_str).map_err(AppError::Notmuch)?;
    let messages = query.search_messages().map_err(AppError::Notmuch)?;

    let mut counts = HashMap::<String, usize>::new();

    for msg in messages {
        if let Ok(Some(from)) = msg.header("From") {
            if let Some(m) = EMAIL_RE.find(&from) {
                let email = m.as_str().to_lowercase();
                *counts.entry(email).or_default() += 1;
            } else {
                // Fallback: use the raw From header, truncated to 60 chars
                // (char-aware to avoid panicking on multi-byte UTF-8).
                let raw = from.trim().to_lowercase();
                let key: String = raw.chars().take(60).collect();
                *counts.entry(key).or_default() += 1;
            }
        }
    }

    let mut senders: Vec<(String, usize)> = counts.into_iter().collect();
    senders.sort_by_key(|s| std::cmp::Reverse(s.1));
    senders.truncate(limit);

    Ok(senders)
}
