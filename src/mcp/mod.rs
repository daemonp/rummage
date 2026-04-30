//! MCP server core for Rummage.
//!
//! Provides a Model Context Protocol interface over the existing notmuch-backed
//! database worker. All business logic is thin — the MCP layer is just an adapter.

use crate::api::thread::MessageDetail;
use crate::db::DbHandle;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    GetPromptRequestParams, GetPromptResult, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, PaginatedRequestParams, ReadResourceResult, ServerCapabilities,
    ServerInfo,
};
use rmcp::service::{MaybeSendFuture, RequestContext};
use rmcp::RoleServer;

pub mod error;
pub mod prompts;
pub mod resources;
pub mod tools;
pub mod util;

/// MCP handler state — cloneable because it only holds an `mpsc::Sender`.
#[derive(Clone)]
pub struct RummageMcpHandler {
    pub db: DbHandle,
    pub instructions: String,
}

impl RummageMcpHandler {
    pub async fn new(db: DbHandle) -> Self {
        let instructions = build_instructions(&db).await;
        Self { db, instructions }
    }
}

// ── Shared body-format helpers ─────────────────────────────────────
// Delegates to the thorough implementations in `crate::mail::body`.

/// Body output format for MCP tools.
#[derive(Debug, Clone, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BodyFormat {
    /// Clean plain text for analysis and summarization (default).
    #[default]
    Text,
    /// Preserves formatting: bold, links, lists, headings.
    Markdown,
    /// Raw sanitized HTML (wastes tokens, rarely needed).
    Html,
}

impl BodyFormat {
    /// Return the format name as a `&str`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Html => "html",
        }
    }
}

/// Apply the requested body format to a `MessageDetail`.
///
/// Prefers the pre-computed `body_text` / `body_markdown` fields (populated by
/// the parser at message-load time).  Falls back to on-the-fly conversion via
/// `mail::body` only when the fields are `None`.
pub(crate) fn format_body(msg: &MessageDetail, body_format: &BodyFormat) -> String {
    match body_format {
        BodyFormat::Html => msg.content.clone(),
        BodyFormat::Markdown => msg
            .body_markdown
            .clone()
            .unwrap_or_else(|| crate::mail::body::html_to_markdown(&msg.content)),
        BodyFormat::Text => msg
            .body_text
            .clone()
            .unwrap_or_else(|| crate::mail::body::html_to_text(&msg.content)),
    }
}

/// Extract a snippet (~200 chars) around a case-insensitive match.
///
/// Uses char-index mapping to avoid panicking on multibyte text: positions
/// found in the lowercased copy are translated back to byte offsets in the
/// original string via a char-boundary walk.
pub(crate) fn extract_snippet(text: &str, term: &str) -> String {
    let lower_text = text.to_lowercase();
    let lower_term = term.to_lowercase();
    if let Some(char_pos) = lower_text.find(&lower_term) {
        // `char_pos` is a byte offset in `lower_text`.  We need the
        // corresponding byte offset in the *original* `text`.  Because
        // `to_lowercase()` can change byte lengths (e.g. 'ß' → "ss"), we
        // walk both strings in lockstep by chars.
        let orig_start = char_byte_offset(text, &lower_text, char_pos);
        let orig_end = char_byte_offset(text, &lower_text, char_pos + lower_term.len());

        let snippet_start = snap_to_char_boundary(text, orig_start.saturating_sub(100));
        let snippet_end = snap_to_char_boundary_end(text, (orig_end + 100).min(text.len()));

        let mut snippet = String::new();
        if snippet_start > 0 {
            snippet.push_str("...");
        }
        snippet.push_str(&text[snippet_start..snippet_end]);
        if snippet_end < text.len() {
            snippet.push_str("...");
        }
        snippet
    } else {
        text.chars().take(200).collect::<String>()
    }
}

/// Map a byte offset in `lowered` back to the corresponding byte offset in
/// `original`.  Both must be derived from the same source (`lowered =
/// original.to_lowercase()`).  We walk char-by-char through both strings in
/// lockstep.
fn char_byte_offset(original: &str, lowered: &str, target_byte_in_lowered: usize) -> usize {
    let mut orig_iter = original.char_indices();
    let mut low_byte = 0usize;

    for lc in lowered.chars() {
        if low_byte >= target_byte_in_lowered {
            break;
        }
        low_byte += lc.len_utf8();
        // Advance original by one char.
        orig_iter.next();
    }

    orig_iter
        .next()
        .map(|(idx, _)| idx)
        .unwrap_or(original.len())
        // If we consumed all of lowered but target is at the end, use original.len().
        .min(original.len())
}

/// Snap a byte offset forward to a char boundary.
fn snap_to_char_boundary(text: &str, mut pos: usize) -> usize {
    while pos < text.len() && !text.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

/// Snap a byte offset backward to a char boundary.
fn snap_to_char_boundary_end(text: &str, mut pos: usize) -> usize {
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

// ── Dynamic instructions ───────────────────────────────────────────

/// Build a rich instruction string by querying the live archive.
pub async fn build_instructions(db: &DbHandle) -> String {
    let stats = match db.stats().await {
        Ok(s) => s,
        Err(_) => {
            return "Rummage email archive MCP server.\n\
                Search uses notmuch query syntax.\n"
                .into();
        }
    };

    let tags = db.tags().await.unwrap_or_default();
    let senders = db.senders().await.unwrap_or_default();

    let top_tags: Vec<String> = tags
        .into_iter()
        .take(10)
        .map(|(name, count)| format!("{name} ({count})"))
        .collect();

    let top_senders: Vec<String> = senders
        .into_iter()
        .take(10)
        .map(|(email, count)| format!("{email} ({count})"))
        .collect();

    format!(
        "This is a read-only email archive containing {total_messages} messages across {total_threads} threads.\n\
         Top tags: {tags}\n\
         Top senders: {senders}\n\n\
         Search uses notmuch query syntax:\n\
           from:alice@example.com         — messages from alice\n\
           tag:inbox AND date:2013-06..   — inbox messages from June 2013 onward\n\
           subject:invoice has:attachment  — invoices with attachments\n\
           \"exact phrase\"                  — phrase search\n\
           NOT tag:spam                    — exclude spam\n\n\
         Strategy:\n\
           - Start broad, then narrow with filters\n\
           - Use tag: and from: for targeted searches\n\
           - Use date: ranges to scope time periods\n\
           - Use has:attachment to find messages with files\n\
                       - For document content, use get_attachment_text to read PDFs, DOCX, XLSX",
        total_messages = stats.total_messages,
        total_threads = stats.total_threads,
        tags = top_tags.join(", "),
        senders = top_senders.join(", "),
    )
}

// ── ServerHandler implementation ───────────────────────────────────

#[rmcp::tool_handler(router = RummageMcpHandler::tool_router())]
#[rmcp::prompt_handler(router = RummageMcpHandler::prompt_router())]
impl ServerHandler for RummageMcpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_instructions(self.instructions.clone())
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, rmcp::ErrorData> {
        Ok(resources::list_resources(&self.db).await)
    }

    async fn list_resource_templates(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourceTemplatesResult, rmcp::ErrorData> {
        Ok(resources::list_resource_templates().await)
    }

    fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResult, rmcp::ErrorData>>
           + MaybeSendFuture
           + '_ {
        let db = self.db.clone();
        let uri = request.uri;
        async move { resources::read_resource(&db, &uri).await }
    }
}

#[cfg(test)]
pub mod tests;
