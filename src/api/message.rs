use crate::db::DbHandle;
use crate::error::AppError;
use crate::error::Result;
use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::response::IntoResponse;
use axum::Json;

/// Download a raw `.eml` message by its notmuch message ID.
///
/// # Errors
/// Returns `AppError::NotFound` if the message does not exist,
/// or `AppError::Io` / `AppError::Notmuch` on underlying failures.
pub async fn handler(
    State(db): State<DbHandle>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse> {
    let body = db.raw_message(id).await?;

    let response = axum::http::Response::builder()
        .header("Content-Type", "message/rfc822")
        .header(
            "Content-Disposition",
            "attachment; filename=\"message.eml\"",
        )
        .body(Body::from(body))
        .map_err(|e| AppError::Internal(format!("response build error: {e}")))?;

    Ok(response)
}

/// Return parsed `MessageDetail` JSON for a single message.
///
/// # Errors
/// Returns `AppError::NotFound` if the message does not exist,
/// or `AppError::Io` / `AppError::Notmuch` / `AppError::MailParse` on underlying failures.
pub async fn detail_handler(
    State(db): State<DbHandle>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<crate::api::thread::MessageDetail>> {
    let detail = db.message_detail(id).await?;
    // `message_detail` returns a ThreadDetail containing one message; extract it.
    let msg = detail
        .messages
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("message not found".into()))?;
    Ok(Json(msg))
}

/// Synchronous raw message extraction against an open notmuch `Database`.
///
/// # Errors
/// Returns `AppError::NotFound` if the message does not exist.
pub fn do_raw_message(db: &notmuch::Database, msg_id: &str) -> Result<Vec<u8>> {
    crate::db::find_message_bytes(db, msg_id)
}

/// Synchronous parsed message detail extraction against an open notmuch `Database`.
///
/// Queries notmuch for the actual message date, tags, and thread ID rather
/// than using placeholder values.
///
/// # Errors
/// Returns `AppError::NotFound` if the message does not exist,
/// or `AppError::Io` / `AppError::MailParse` on underlying failures.
pub fn do_message_detail(
    db: &notmuch::Database,
    msg_id: &str,
) -> Result<crate::api::thread::ThreadDetail> {
    let query = db
        .create_query(&format!("id:{msg_id}"))
        .map_err(AppError::Notmuch)?;
    let mut msgs = query.search_messages().map_err(AppError::Notmuch)?;
    let msg = msgs
        .next()
        .ok_or_else(|| AppError::NotFound(format!("message not found: {msg_id}")))?;

    let date = msg.date();
    let date_relative = chrono::DateTime::from_timestamp(date, 0).map_or_else(
        || "unknown".into(),
        |dt| dt.format("%Y-%m-%d %H:%M").to_string(),
    );
    let tags: Vec<String> = msg.tags().collect();
    let thread_id = msg.thread_id().to_string();

    let filename = msg.filename();
    let bytes = std::fs::read(filename).map_err(AppError::Io)?;

    let detail = crate::mail::parser::parse_message(&bytes, msg_id, date, date_relative, tags)?;
    Ok(crate::api::thread::ThreadDetail {
        thread_id,
        tags: Vec::new(),
        messages: vec![detail],
    })
}
