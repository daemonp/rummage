pub mod body;
pub mod parser;

use crate::api::thread::MessageDetail;
use crate::error::{AppError, Result};
use notmuch::Message;

/// Extract message headers, body, and attachments from a notmuch `Message`.
///
/// # Errors
/// Returns `AppError::Io` if the underlying mail file cannot be read,
/// or `AppError::MailParse` if the message fails to parse.
pub fn extract_message(msg: &Message) -> Result<MessageDetail> {
    let message_id = msg.id().to_string();
    let date = msg.date();
    let date_relative = chrono::DateTime::from_timestamp(date, 0).map_or_else(
        || "unknown".into(),
        |dt| dt.format("%Y-%m-%d %H:%M").to_string(),
    );

    let tags: Vec<String> = msg.tags().collect();

    let filename = msg.filename();
    let raw = std::fs::read(filename).map_err(AppError::Io)?;

    parser::parse_message(&raw, &message_id, date, date_relative, tags)
}

/// Extract a single MIME part from raw message bytes, returning
/// `(content_type, body, filename)` in a single parse pass.
///
/// # Errors
/// Returns `AppError::NotFound` if the part does not exist,
/// or `AppError::MailParse` if the message fails to parse.
pub fn extract_attachment_full(
    raw: &[u8],
    part_num: usize,
) -> Result<(String, Vec<u8>, Option<String>)> {
    parser::extract_attachment_full(raw, part_num)
}

/// Extract a single MIME part from raw message bytes.
///
/// # Errors
/// Returns `AppError::NotFound` if the part does not exist,
/// or `AppError::MailParse` if the message fails to parse.
pub fn extract_attachment(raw: &[u8], part_num: usize) -> Result<(String, Vec<u8>)> {
    let (ct, body, _filename) = parser::extract_attachment_full(raw, part_num)?;
    Ok((ct, body))
}

/// Attempt to resolve the filename of a MIME part.
#[must_use]
pub fn attachment_filename(raw: &[u8], part_num: usize) -> Option<String> {
    parser::attachment_filename(raw, part_num)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_real_email() {
        // This test requires the mail archive to be present; skip if absent.
        let mail_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("mail/test-archive/cur");
        let path = mail_dir.join("1644854736.M130629P206164Q20045.nuc:2,S");
        if !path.exists() {
            return;
        }
        let raw = std::fs::read(path).unwrap();
        let detail = parser::parse_message(&raw, "test-id", 0, "now".into(), Vec::new()).unwrap();
        assert!(
            !detail.headers.from.is_empty(),
            "from header should not be empty: {:?}",
            detail.headers.from
        );
        assert!(
            !detail.headers.to.is_empty(),
            "to header should not be empty: {:?}",
            detail.headers.to
        );
    }
}
