//! Email MIME parsing — headers, body extraction, and attachment handling.

use crate::api::thread::{AttachmentSummary, MessageDetail, MessageHeaders};
use crate::error::{AppError, Result};
use mail_parser::{HeaderValue, MessageParser, MessagePart, MimeHeaders, PartType};

/// Parse raw RFC822 bytes into a `MessageDetail` struct.
///
/// # Errors
/// Returns `AppError::MailParse` if the message cannot be parsed,
/// or `AppError::Io` if a file referenced by the message is missing.
pub fn parse_message(
    raw: &[u8],
    message_id: &str,
    date: i64,
    date_relative: String,
    tags: Vec<String>,
) -> Result<MessageDetail> {
    let parser = MessageParser::default();
    let parsed = parser
        .parse(raw)
        .ok_or_else(|| AppError::MailParse("failed to parse email".into()))?;

    let from = parsed.from().map(format_address_list).unwrap_or_default();
    let to = parsed.to().map(format_address_list).unwrap_or_default();
    let cc = parsed.cc().map(format_address_list);
    let bcc = parsed.bcc().map(format_address_list);
    let subject = parsed.subject().unwrap_or("(no subject)").to_string();

    let (content, content_type, attachments, in_reply_to, references, raw_text) =
        extract_body_and_attachments(&parsed);

    let (body_text, body_markdown) = match content_type.as_str() {
        "text/html" => (
            Some(crate::mail::body::html_to_text(&content)),
            Some(crate::mail::body::html_to_markdown(&content)),
        ),
        "text/plain" => {
            let text = raw_text.unwrap_or_else(|| content.clone());
            (
                Some(text.clone()),
                Some(text), // light formatting: just plain text for now
            )
        }
        _ => (None, None),
    };

    Ok(MessageDetail {
        message_id: message_id.to_string(),
        headers: MessageHeaders { from, to, cc, bcc },
        date,
        date_relative,
        subject,
        content,
        content_type,
        attachments,
        body_text,
        body_markdown,
        tags,
        in_reply_to,
        references,
    })
}

/// Extract the preferred body, all attachments, and threading headers from a parsed message.
///
/// Returns `(content, content_type, attachments, in_reply_to, references, raw_text_body)`.
fn extract_body_and_attachments(
    parsed: &mail_parser::Message,
) -> (
    String,
    String,
    Vec<AttachmentSummary>,
    Option<String>,
    Vec<String>,
    Option<String>,
) {
    let mut html_body: Option<String> = None;
    let mut text_body: Option<String> = None;
    let mut attachments = Vec::new();

    // html_body and text_body are Vec<u32> indices into parts
    if let Some(&idx) = parsed.html_body.first() {
        if let Some(part) = parsed.parts.get(idx as usize) {
            if let PartType::Text(text) = &part.body {
                html_body = Some(text.to_string());
            }
        }
    }

    if let Some(&idx) = parsed.text_body.first() {
        if let Some(part) = parsed.parts.get(idx as usize) {
            if let PartType::Text(text) = &part.body {
                text_body = Some(text.to_string());
            }
        }
    }

    // Collect attachments from attachment indices
    for &idx in &parsed.attachments {
        if let Some(part) = parsed.parts.get(idx as usize) {
            attachments.push(attachment_summary(part, idx as usize));
        }
    }

    // If no attachments were found via indices, scan all non-body parts
    if attachments.is_empty() {
        for (idx, part) in parsed.parts.iter().enumerate() {
            if parsed.html_body.contains(&(idx as u32)) || parsed.text_body.contains(&(idx as u32))
            {
                continue;
            }
            if let PartType::Binary(_) | PartType::InlineBinary(_) = &part.body {
                attachments.push(attachment_summary(part, idx));
            }
        }
    }

    let raw_text = text_body.clone();

    let (content, content_type) = html_body.map_or_else(
        || {
            text_body.map_or_else(
                || ("(no readable body)".into(), "text/plain".into()),
                |text| (crate::mail::body::text_to_html(&text), "text/plain".into()),
            )
        },
        |html| (crate::mail::body::sanitize_html(&html), "text/html".into()),
    );

    let in_reply_to = parsed.header("In-Reply-To").and_then(|h| match h {
        HeaderValue::Text(t) => Some(t.to_string()),
        _ => None,
    });
    let references: Vec<String> = parsed
        .header("References")
        .map(|h| match h {
            HeaderValue::TextList(list) => list
                .iter()
                .flat_map(|s| s.split_whitespace())
                .map(String::from)
                .collect(),
            HeaderValue::Text(t) => t.split_whitespace().map(String::from).collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default();

    (
        content,
        content_type,
        attachments,
        in_reply_to,
        references,
        raw_text,
    )
}

// ── MIME helpers ────────────────────────────────────────────────────

/// Build an `AttachmentSummary` from a MIME part.
fn attachment_summary(part: &MessagePart, idx: usize) -> AttachmentSummary {
    let fname = filename_of(part);
    let ct = refine_content_type(&content_type_of(part), fname.as_deref());
    let size_bytes = match &part.body {
        PartType::Binary(bin) | PartType::InlineBinary(bin) => Some(bin.len()),
        PartType::Text(text) => Some(text.len()),
        _ => None,
    };
    AttachmentSummary {
        filename: fname,
        content_type: ct,
        part: idx,
        size_bytes,
    }
}

/// If the MIME type is the generic `application/octet-stream`, try to infer
/// a more specific type from the filename extension.  Many email clients
/// send PDFs/images with this generic type.
fn refine_content_type(ct: &str, filename: Option<&str>) -> String {
    if ct == "application/octet-stream" {
        if let Some(name) = filename {
            if let Some(guessed) = mime_guess::from_path(name).first() {
                return guessed.to_string();
            }
        }
    }
    ct.to_string()
}

/// Resolve the MIME content-type string for a part.
fn content_type_of(part: &MessagePart) -> String {
    part.content_type().map_or_else(
        || "application/octet-stream".into(),
        |ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("octet-stream")),
    )
}

/// Resolve the filename (or `name` attribute) for a MIME part.
fn filename_of(part: &MessagePart) -> Option<String> {
    part.content_disposition()
        .and_then(|cd| {
            cd.attribute("filename")
                .map(std::string::ToString::to_string)
        })
        .or_else(|| {
            part.content_type()
                .and_then(|ct| ct.attribute("name").map(std::string::ToString::to_string))
        })
}

// ── Attachment extraction ──────────────────────────────────────────

/// Extract a single MIME part by index, returning content-type, body, and
/// filename in one parse pass (avoiding the double-parse of the old
/// `extract_attachment` + `attachment_filename` pair).
///
/// # Errors
/// Returns `AppError::NotFound` if the part index is out of bounds,
/// or `AppError::MailParse` if the message cannot be parsed.
pub fn extract_attachment_full(
    raw: &[u8],
    part_num: usize,
) -> Result<(String, Vec<u8>, Option<String>)> {
    let parser = MessageParser::default();
    let parsed = parser
        .parse(raw)
        .ok_or_else(|| AppError::MailParse("failed to parse email for attachment".into()))?;

    let part = parsed
        .parts
        .get(part_num)
        .ok_or_else(|| AppError::NotFound(format!("part {part_num} not found")))?;

    let fname = filename_of(part);
    let ct = refine_content_type(&content_type_of(part), fname.as_deref());

    let body = match &part.body {
        PartType::Binary(bin) | PartType::InlineBinary(bin) => bin.to_vec(),
        PartType::Text(text) => text.as_bytes().to_vec(),
        _ => return Err(AppError::NotFound("part is not a binary attachment".into())),
    };

    Ok((ct, body, fname))
}

/// Attempt to resolve the filename of a MIME part by index.
#[must_use]
pub fn attachment_filename(raw: &[u8], part_num: usize) -> Option<String> {
    let parser = MessageParser::default();
    let parsed = parser.parse(raw)?;
    let part = parsed.parts.get(part_num)?;
    filename_of(part)
}

// ── Address formatting ─────────────────────────────────────────────

fn format_address(addr: &mail_parser::Addr) -> String {
    let address = addr.address().unwrap_or("(no address)");
    if let Some(name) = addr.name() {
        format!("{name} <{address}>")
    } else {
        address.to_string()
    }
}

fn format_address_list(addrs: &mail_parser::Address) -> String {
    match addrs {
        mail_parser::Address::List(list) => list
            .iter()
            .map(format_address)
            .collect::<Vec<_>>()
            .join(", "),
        mail_parser::Address::Group(groups) => groups
            .iter()
            .flat_map(|g| g.addresses.iter().map(format_address))
            .collect::<Vec<_>>()
            .join(", "),
    }
}
