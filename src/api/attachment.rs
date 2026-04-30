use crate::db::DbHandle;
use crate::error::{AppError, Result};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use notmuch::Database;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AttachmentParams {
    pub msg: String,
    pub part: usize,
}

/// Download an email attachment by message ID and part number.
///
/// # Errors
/// Returns `AppError::NotFound` if the message or part does not exist,
/// or `AppError::Io` / `AppError::Notmuch` on underlying failures.
pub async fn handler(
    State(db): State<DbHandle>,
    Query(params): Query<AttachmentParams>,
) -> Result<impl IntoResponse> {
    let data = db.attachment(params.msg, params.part).await?;

    // Use Content-Disposition: inline for types that can render in the browser
    // (PDFs, images, text, video), and "attachment" for everything else.
    let ct = &data.content_type;
    let disposition = if ct.starts_with("image/")
        || ct.starts_with("text/")
        || ct.starts_with("video/")
        || ct == "application/pdf"
    {
        "inline"
    } else {
        "attachment"
    };
    // Sanitize filename to prevent header injection — strip control chars and quotes.
    let raw_fname = data.filename.as_deref().unwrap_or("attachment");
    let fname: String = raw_fname
        .chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
        .collect();
    let fname = if fname.is_empty() {
        "attachment".to_string()
    } else {
        fname
    };
    let disposition_header = format!("{disposition}; filename=\"{fname}\"");

    let response = axum::http::Response::builder()
        .header("Content-Type", data.content_type)
        .header("Content-Disposition", disposition_header)
        .body(Body::from(data.body))
        .map_err(|e| AppError::Internal(format!("response build error: {e}")))?;

    Ok(response)
}

/// Synchronous attachment extraction against an open notmuch `Database`.
///
/// # Errors
/// Returns `AppError::NotFound` if the message or part does not exist.
pub fn do_attachment(
    db: &Database,
    msg_id: &str,
    part_num: usize,
) -> Result<crate::db::AttachmentData> {
    let bytes = crate::db::find_message_bytes(db, msg_id)?;
    let (content_type, body, filename) = crate::mail::extract_attachment_full(&bytes, part_num)?;

    Ok(crate::db::AttachmentData {
        content_type,
        body,
        filename,
    })
}

/// Extract readable text from an attachment using `batdoc-core` for binary
/// formats and direct UTF-8 decoding for text-based ones.
///
/// # Errors
/// Returns `AppError::NotFound` if the message or part does not exist,
/// or `AppError::Internal` if text extraction fails.
pub fn do_attachment_text(
    db: &Database,
    msg_id: &str,
    part: usize,
    format: &str,
) -> Result<String> {
    let bytes = crate::db::find_message_bytes(db, msg_id)?;
    let (content_type, body, filename) = crate::mail::extract_attachment_full(&bytes, part)?;

    // For text-based attachments, decode directly without batdoc-core.
    let is_text = content_type.starts_with("text/")
        || filename.as_ref().is_some_and(|f| {
            let lower = f.to_lowercase();
            lower.ends_with(".txt")
                || lower.ends_with(".csv")
                || lower.ends_with(".json")
                || lower.ends_with(".html")
                || lower.ends_with(".htm")
                || lower.ends_with(".md")
                || lower.ends_with(".markdown")
                || lower.ends_with(".xml")
                || lower.ends_with(".yaml")
                || lower.ends_with(".yml")
                || lower.ends_with(".rst")
                || lower.ends_with(".log")
        });

    let text = if is_text {
        String::from_utf8_lossy(&body).into_owned()
    } else {
        // Try batdoc-core format detection and extraction.
        match batdoc_core::detect_format(&body) {
            Ok(fmt) => match format {
                "markdown" => batdoc_core::extract_markdown(&body, fmt, false).map_err(|e| {
                    AppError::Internal(format!(
                        "batdoc markdown extraction failed for {content_type}: {e:?}"
                    ))
                })?,
                _ => batdoc_core::extract_plain(&body, fmt).map_err(|e| {
                    AppError::Internal(format!(
                        "batdoc plain extraction failed for {content_type}: {e:?}"
                    ))
                })?,
            },
            Err(_) => {
                // Format not recognized by batdoc-core — fall back to UTF-8 lossy decode
                // so the user at least sees something.
                String::from_utf8_lossy(&body).into_owned()
            }
        }
    };

    Ok(text)
}
