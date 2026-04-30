//! MCP tool implementations — 13 tools for email archive exploration.

use crate::api::search::ThreadSummary;
use crate::error::AppError;
use crate::mcp::BodyFormat;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use tracing::{info_span, trace};

// ── Parameter structs ──────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Notmuch query (e.g., "from:alice tag:inbox").
    pub query: String,
    /// Max threads to return (default: 20, max: 100).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Pagination offset (default: 0).
    #[serde(default)]
    pub offset: Option<usize>,
    /// Sort order: "newest" or "oldest" (default: "newest").
    #[serde(default)]
    pub sort: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetThreadParams {
    /// Thread ID from search results.
    pub thread_id: String,
    /// Body output format (default: text).
    #[serde(default)]
    pub body_format: BodyFormat,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetMessageParams {
    /// Message ID (notmuch message ID).
    pub message_id: String,
    /// Body output format (default: text).
    #[serde(default)]
    pub body_format: BodyFormat,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAttachmentParams {
    /// Message ID containing the attachment.
    pub message_id: String,
    /// MIME part number (from attachment listings).
    pub part: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAttachmentTextParams {
    /// Message ID containing the attachment.
    pub message_id: String,
    /// MIME part number (from attachment listings).
    pub part: usize,
    /// Output format: "text" or "markdown" (default: "markdown").
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTagsParams {
    /// Max tags to return (default: 50).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListSendersParams {
    /// Max senders to return (default: 20).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Optional query to scope senders.
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchWithinThreadParams {
    /// Thread ID to search inside.
    pub thread_id: String,
    /// Case-insensitive search term.
    pub term: String,
    /// Body output format (default: text).
    #[serde(default)]
    pub body_format: BodyFormat,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindRelatedThreadsParams {
    /// Source thread ID.
    pub thread_id: String,
    /// Strategy: "participants", "subject", "tags", or "auto" (default: "auto").
    #[serde(default)]
    pub strategy: Option<String>,
    /// Max related threads (default: 10).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetConversationTreeParams {
    /// Thread ID.
    pub thread_id: String,
    /// Body output format (default: text).
    #[serde(default)]
    pub body_format: BodyFormat,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CountResultsParams {
    /// Notmuch query.
    pub query: String,
}

use super::RummageMcpHandler;

/// Strip `Re:` and `Fwd:` prefixes from a subject line and trim whitespace.
fn strip_reply_prefixes(subject: &str) -> String {
    subject
        .replace("Re:", "")
        .replace("Fwd:", "")
        .trim()
        .to_string()
}

// ── Tool implementation block ──────────────────────────────────────

#[rmcp::tool_router(router = tool_router, vis = "pub")]
impl RummageMcpHandler {
    /// Search the email archive using notmuch query syntax. Returns paginated thread
    /// summaries with preview snippets.
    ///
    /// Use this tool for:
    ///   - Finding emails by sender, recipient, subject, or content
    ///   - Browsing threads with attachments
    ///   - Exploring messages from a time period
    ///
    /// Query syntax:
    ///   from:alice@example.com           — messages from a sender
    ///   to:bob@example.com               — messages to a recipient
    ///   subject:"quarterly report"       — exact phrase in subject
    ///   tag:inbox                        — messages with a tag
    ///   has:attachment                   — messages with attachments
    ///   date:2013-06-01..2013-06-30     — date range (YYYY-MM-DD)
    ///   "exact phrase"                   — full-text phrase search
    ///   from:alice AND tag:important     — boolean AND
    ///   from:alice OR from:bob           — boolean OR
    ///   NOT tag:spam                     — negation
    ///
    /// Strategy by goal:
    ///   Goal: Find specific emails     → from: + subject: + date: range
    ///   Goal: Browse a topic             → free text or "phrase" search
    ///   Goal: Find attachments           → has:attachment + other filters
    ///   Goal: Explore a time period      → date: range, then refine with tag:/from:
    ///
    /// Parameters:
    ///   query  — notmuch query string (required)
    ///   limit  — max threads to return (default: 20, max: 100)
    ///   offset — pagination offset (default: 0)
    ///   sort   — "newest" or "oldest" (default: "newest")
    ///
    /// Example call:
    ///   { "query": "from:alice@example.com tag:inbox date:2013-06..", "limit": 10 }
    ///
    /// Returns: total_results, threads[] with thread_id, subject, authors, date,
    /// matched_messages, total_messages, tags[], preview, has_attachments.
    ///
    /// Tips:
    ///   - Start broad (20 results), scan subjects/previews, then narrow.
    ///   - Use offset for more results. Total count is always returned.
    ///   - Preview snippets are ~200 chars from the first matching message.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn search(&self, Parameters(params): Parameters<SearchParams>) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "search", query = %params.query);
        let _timer = std::time::Instant::now();

        if params.query.trim().is_empty() {
            return crate::mcp::error::empty_query_error();
        }

        let limit = params.limit.map(|l| l.min(100));
        let offset = params.offset;
        let sort = params.sort.clone();

        let result = match self
            .db
            .search(params.query.clone(), offset, limit, sort)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return crate::mcp::error::error_result(format!("Search failed: {e}"));
            }
        };

        let total = result.total_count;
        if total == 0 {
            let tags = match self.db.tags().await {
                Ok(t) => t.into_iter().map(|(name, _)| name).collect::<Vec<_>>(),
                Err(_) => Vec::new(),
            };
            let senders = match self.db.senders().await {
                Ok(s) => s.into_iter().map(|(email, _)| email).collect::<Vec<_>>(),
                Err(_) => Vec::new(),
            };
            return crate::mcp::error::no_results_error(&params.query, &tags, &senders);
        }

        let capped_threads = crate::mcp::util::cap_search_results(result.threads);
        let returned = capped_threads.len();
        let offset_val = params.offset.unwrap_or(0);

        let human = format!(
            "Found {total} thread(s) for query '{}'. Showing {}–{}.",
            params.query,
            offset_val + 1,
            offset_val + returned,
        );

        let structured = json!({
            "query": params.query,
            "total_results": total,
            "offset": offset_val,
            "limit": params.limit.unwrap_or(20),
            "threads": capped_threads,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Retrieve all messages in a conversation thread with full content.
    ///
    /// Use this tool to read a complete email thread after discovering it via search.
    /// Messages are ordered chronologically within the thread.
    ///
    /// Parameters:
    ///   thread_id    — Thread ID from search results (required)
    ///   body_format  — Output format: "text" (default), "markdown", or "html"
    ///
    /// Format guidance:
    ///   "text"     — Plain text, best for analysis and summarization (default)
    ///   "markdown" — Preserves formatting: bold, links, lists, quote blocks
    ///   "html"     — Raw sanitized HTML (wastes tokens, rarely needed)
    ///
    /// Returns: thread_id, subject, tags, total_messages, messages[] with
    /// message_id, from, to, cc, date, subject, body, attachments[].
    ///
    /// Tips:
    ///   - For long threads, bodies may be truncated; use get_message for full text.
    ///   - Attachment metadata is included; use get_attachment_text to read documents.
    ///   - Use search_within_thread to locate specific content before fetching all messages.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn get_thread(
        &self,
        Parameters(params): Parameters<GetThreadParams>,
    ) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "get_thread", thread_id = %params.thread_id);
        let _timer = std::time::Instant::now();

        let body_format = &params.body_format;

        let mut thread = match self.db.thread(params.thread_id.clone()).await {
            Ok(t) => t,
            Err(AppError::NotFound(_)) => {
                return crate::mcp::error::not_found_error("Thread", &params.thread_id);
            }
            Err(e) => {
                return crate::mcp::error::error_result(format!("Failed to fetch thread: {e}"))
            }
        };

        for msg in &mut thread.messages {
            msg.content = crate::mcp::util::format_and_truncate_body(msg, body_format);
            if let Some(ref mut bt) = msg.body_text {
                *bt = crate::mcp::util::truncate_thread_body(bt);
            }
            if let Some(ref mut bm) = msg.body_markdown {
                *bm = crate::mcp::util::truncate_thread_body(bm);
            }
        }

        let human = format!(
            "Thread '{}' contains {} message(s).",
            thread
                .messages
                .first()
                .map(|m| m.subject.as_str())
                .unwrap_or("(no subject)"),
            thread.messages.len()
        );

        let structured = json!({
            "thread_id": thread.thread_id,
            "tags": thread.tags,
            "total_messages": thread.messages.len(),
            "messages": thread.messages,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Get a single message by its notmuch message ID.
    ///
    /// Use this tool when you need full content of one specific message,
    /// rather than an entire thread. Useful for large threads where only
    /// one message is relevant.
    ///
    /// Parameters:
    ///   message_id   — Notmuch message ID (required)
    ///   body_format  — "text" (default), "markdown", or "html"
    ///
    /// Format guidance:
    ///   "text"     — Clean plain text for analysis and summarization (default)
    ///   "markdown" — Preserves formatting, headings, links, lists
    ///   "html"     — Raw HTML, only needed when rendering for display
    ///
    /// Returns: message_id, from, to, cc, bcc, date, subject, body,
    /// body_format, attachments[] with filename, content_type, part, size_bytes.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn get_message(
        &self,
        Parameters(params): Parameters<GetMessageParams>,
    ) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "get_message", message_id = %params.message_id);
        let _timer = std::time::Instant::now();

        let body_format = &params.body_format;

        let detail = match self.db.message_detail(params.message_id.clone()).await {
            Ok(d) => d,
            Err(AppError::NotFound(_)) => {
                return crate::mcp::error::not_found_error("Message", &params.message_id);
            }
            Err(e) => {
                return crate::mcp::error::error_result(format!("Failed to fetch message: {e}"));
            }
        };

        let mut msg = match detail.messages.into_iter().next() {
            Some(m) => m,
            None => {
                return crate::mcp::error::not_found_error("Message", &params.message_id);
            }
        };

        let body = crate::mcp::util::format_and_truncate_body(&msg, body_format);
        if let Some(ref mut bt) = msg.body_text {
            *bt = crate::mcp::util::truncate_thread_body(bt);
        }
        if let Some(ref mut bm) = msg.body_markdown {
            *bm = crate::mcp::util::truncate_thread_body(bm);
        }

        let human = format!(
            "Message from {} — {} — {}.",
            msg.headers.from, msg.subject, msg.date_relative
        );

        let structured = json!({
            "message_id": msg.message_id,
            "from": msg.headers.from,
            "to": msg.headers.to,
            "cc": msg.headers.cc,
            "bcc": msg.headers.bcc,
            "date": msg.date_relative,
            "subject": msg.subject,
            "body": body,
            "body_format": body_format.as_str(),
            "attachments": msg.attachments,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Download a raw attachment as base64-encoded binary data.
    ///
    /// Use this tool for non-text attachments (images, archives, executables)
    /// or when you need the original bytes. For document attachments (PDF,
    /// DOCX, XLSX, etc.), prefer get_attachment_text to extract readable text.
    ///
    /// Parameters:
    ///   message_id — Message ID containing the attachment (required)
    ///   part       — MIME part number from attachment listings (required)
    ///
    /// Returns: filename, content_type, size_bytes, base64-encoded body.
    ///
    /// Note: The base64 string can be large; only call this when you need
    /// the raw binary, not for reading document content.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn get_attachment(
        &self,
        Parameters(params): Parameters<GetAttachmentParams>,
    ) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "get_attachment", message_id = %params.message_id, part = params.part);
        let _timer = std::time::Instant::now();

        let data = match self
            .db
            .attachment(params.message_id.clone(), params.part)
            .await
        {
            Ok(d) => d,
            Err(AppError::NotFound(_)) => {
                return crate::mcp::error::not_found_error(
                    "Attachment part",
                    &format!("{}/part {}", params.message_id, params.part),
                );
            }
            Err(e) => {
                return crate::mcp::error::error_result(format!("Failed to fetch attachment: {e}"));
            }
        };

        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data.body);

        let human = format!(
            "Attachment '{}' ({}, {} bytes) encoded as base64.",
            data.filename.as_deref().unwrap_or("unnamed"),
            data.content_type,
            data.body.len()
        );

        let structured = json!({
            "message_id": params.message_id,
            "part": params.part,
            "filename": data.filename,
            "content_type": data.content_type,
            "size_bytes": data.body.len(),
            "base64": b64,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Extract readable text from a document attachment.
    ///
    /// Use this tool to read the content of attached documents without
    /// downloading raw binary. Supports a wide range of office and text formats.
    ///
    /// Supported formats:
    ///   .docx  — Word documents (headings, tables, lists preserved in markdown)
    ///   .xlsx  — Excel spreadsheets (rendered as markdown tables, per sheet)
    ///   .pdf   — Text-based PDFs (extracts text content; no OCR for scanned pages)
    ///   .pptx  — PowerPoint presentations (text + heading inference)
    ///   .doc   — Legacy Word 97+ binary format (heuristic structure inference)
    ///   .xls   — Legacy Excel 97+ binary format (BIFF8 parser)
    ///   .txt, .csv, .json, .html, .md — Returned as-is with optional markdown wrapping
    ///
    /// Parameters:
    ///   message_id — Message ID containing the attachment (required)
    ///   part       — MIME part number (required)
    ///   format     — "markdown" (default) or "text"
    ///
    /// Format guidance:
    ///   "markdown" (default) — Preserves structure: headings, tables, bold, links
    ///   "text"               — Flat plain text, minimal formatting
    ///
    /// Returns: extracted text/markdown content.
    ///
    /// Tip: For scanned/image-only PDFs, text extraction will fail — the PDF
    /// contains no text layer. Use get_attachment for the raw binary instead.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn get_attachment_text(
        &self,
        Parameters(params): Parameters<GetAttachmentTextParams>,
    ) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "get_attachment_text", message_id = %params.message_id, part = params.part);
        let _timer = std::time::Instant::now();

        let format = params.format.as_deref().unwrap_or("markdown");
        let text = match self
            .db
            .attachment_text(params.message_id.clone(), params.part, format.to_string())
            .await
        {
            Ok(t) => crate::mcp::util::limit_attachment_text(&t),
            Err(AppError::Unsupported(msg)) => {
                return crate::mcp::error::unsupported_format_error(&msg);
            }
            Err(e) => {
                return crate::mcp::error::error_result(format!(
                    "Attachment text extraction failed: {e}"
                ));
            }
        };

        let human = format!(
            "Extracted {} text from attachment part {} of message {}.",
            format, params.part, params.message_id
        );

        let structured = json!({
            "message_id": params.message_id,
            "part": params.part,
            "format": format,
            "content": text,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// List all tags in the archive with message counts.
    ///
    /// Use this tool for archive orientation — tags often mirror email folders
    /// or labels (e.g., inbox, sent, important, attachment). Tag counts reveal
    /// which categories are most populated.
    ///
    /// Parameters:
    ///   limit — Max tags to return (default: 50)
    ///
    /// Returns: total tag count, tags[] with name and message count.
    ///
    /// Strategy: After calling archive_overview, use list_tags to see the
    /// full tag distribution. Then search with tag: filters to narrow results.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn list_tags(
        &self,
        Parameters(params): Parameters<ListTagsParams>,
    ) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "list_tags");
        let _timer = std::time::Instant::now();

        let limit = params.limit.unwrap_or(50);

        let tags = match self.db.tags().await {
            Ok(t) => t,
            Err(e) => return crate::mcp::error::error_result(format!("Failed to list tags: {e}")),
        };

        let total = tags.len();
        let trimmed: Vec<_> = tags.into_iter().take(limit).collect();

        let human = format!("{total} tag(s) in archive. Showing top {}.", trimmed.len());

        let structured = json!({
            "total": total,
            "tags": trimmed,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// List top senders in the archive by message count.
    ///
    /// Use this tool to discover frequent correspondents and key contributors.
    /// Helps identify who dominates the archive before running targeted searches.
    ///
    /// Parameters:
    ///   limit — Max senders to return (default: 20)
    ///   query — Optional notmuch query to scope senders (e.g., "date:2013-06..")
    ///
    /// Returns: senders[] with email address and message count.
    ///
    /// Tip: Combine with a date: query to find top senders in a specific period.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn list_senders(
        &self,
        Parameters(params): Parameters<ListSendersParams>,
    ) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "list_senders");
        let _timer = std::time::Instant::now();

        let limit = params.limit.unwrap_or(20);

        let senders = match self
            .db
            .senders_with_query(params.query.clone(), limit)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                return crate::mcp::error::error_result(format!("Failed to list senders: {e}"))
            }
        };

        let human = format!("Top {} sender(s) by message count.", senders.len());

        let structured = json!({
            "senders": senders,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Get archive-wide statistics and metadata.
    ///
    /// Use this tool for a quick health check: total messages, threads,
    /// tag count, and date range. Helps set expectations before searching.
    ///
    /// Returns: total_messages, total_threads, tag_count, date_range
    /// { oldest, newest }.
    ///
    /// Tip: Call archive_overview instead if you also want tags and senders.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn get_stats(&self) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "get_stats");
        let _timer = std::time::Instant::now();

        let stats = match self.db.stats().await {
            Ok(s) => s,
            Err(e) => return crate::mcp::error::error_result(format!("Failed to get stats: {e}")),
        };

        let human = format!(
            "Archive contains {} messages across {} threads, with {} tag(s).",
            stats.total_messages, stats.total_threads, stats.tag_count
        );

        let structured = json!({
            "total_messages": stats.total_messages,
            "total_threads": stats.total_threads,
            "tag_count": stats.tag_count,
            "date_range": stats.date_range,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Find messages within a specific thread matching a search term.
    ///
    /// Use this tool to locate specific content inside long conversations
    /// without reading every message. Searches subject, sender, and body
    /// text with case-insensitive substring matching.
    ///
    /// Parameters:
    ///   thread_id    — Thread ID to search inside (required)
    ///   term         — Case-insensitive search term (required)
    ///   body_format  — "text" (default) or "markdown"
    ///
    /// Returns: total_in_thread, matches[] with message_id, from, date,
    /// subject, snippet (~200 chars of context), body.
    ///
    /// Strategy: Use this after get_thread when a conversation is too long
    /// to scan manually. The snippet shows context around each match.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn search_within_thread(
        &self,
        Parameters(params): Parameters<SearchWithinThreadParams>,
    ) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "search_within_thread", thread_id = %params.thread_id, term = %params.term);
        let _timer = std::time::Instant::now();

        let body_format = &params.body_format;
        let term = params.term.to_lowercase();

        let thread = match self.db.thread(params.thread_id.clone()).await {
            Ok(t) => t,
            Err(AppError::NotFound(_)) => {
                return crate::mcp::error::not_found_error("Thread", &params.thread_id);
            }
            Err(e) => {
                return crate::mcp::error::error_result(format!("Failed to fetch thread: {e}"));
            }
        };

        let mut matches = Vec::new();
        for msg in &thread.messages {
            let body_text = msg
                .body_text
                .clone()
                .unwrap_or_else(|| crate::mail::body::html_to_text(&msg.content));
            if body_text.to_lowercase().contains(&term)
                || msg.subject.to_lowercase().contains(&term)
                || msg.headers.from.to_lowercase().contains(&term)
            {
                let snippet = crate::mcp::extract_snippet(&body_text, &params.term);
                matches.push(json!({
                    "message_id": msg.message_id,
                    "from": msg.headers.from,
                    "date": msg.date_relative,
                    "subject": msg.subject,
                    "snippet": snippet,
                    "body": crate::mcp::util::format_and_truncate_body(msg, body_format),
                }));
            }
        }

        let human = format!(
            "Found {} match(es) for '{}' in thread '{}'.",
            matches.len(),
            params.term,
            thread
                .messages
                .first()
                .map(|m| m.subject.as_str())
                .unwrap_or("(no subject)")
        );

        let structured = json!({
            "thread_id": params.thread_id,
            "term": params.term,
            "total_in_thread": thread.messages.len(),
            "matches": matches,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Find threads related to a given thread by participants, subject, or tags.
    ///
    /// Use this tool to discover adjacent conversations that share people,
    /// topics, or labels with a thread you already found.
    ///
    /// Strategies:
    ///   "participants" — Threads involving the same From/To addresses
    ///   "subject"      — Threads with similar subject lines (Re:/Fwd: stripped)
    ///   "tags"         — Threads sharing the same tags
    ///   "auto" (default) — Tries participants first, falls back to subject
    ///                      if fewer than 3 results are found
    ///
    /// Parameters:
    ///   thread_id — Source thread ID (required)
    ///   strategy  — "participants", "subject", "tags", or "auto" (default)
    ///   limit     — Max related threads (default: 10)
    ///
    /// Returns: related threads[] with same shape as search results.
    ///
    /// Tip: Override "auto" with "subject" when you want topic breadth,
    /// or "tags" when exploring a labeled category.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn find_related_threads(
        &self,
        Parameters(params): Parameters<FindRelatedThreadsParams>,
    ) -> CallToolResult {
        let _span =
            info_span!("mcp_tool", tool = "find_related_threads", thread_id = %params.thread_id);
        let _timer = std::time::Instant::now();

        let strategy = params.strategy.as_deref().unwrap_or("auto");
        let limit = params.limit.unwrap_or(10);

        let source = match self.db.thread(params.thread_id.clone()).await {
            Ok(t) => t,
            Err(AppError::NotFound(_)) => {
                return crate::mcp::error::not_found_error("Thread", &params.thread_id);
            }
            Err(e) => {
                return crate::mcp::error::error_result(format!("Failed to fetch thread: {e}"));
            }
        };

        // Gather participant emails
        let mut emails = std::collections::HashSet::new();
        for msg in &source.messages {
            for addr in msg.headers.from.split(',') {
                let addr = addr.trim();
                if !addr.is_empty() {
                    emails.insert(addr.to_string());
                }
            }
            for addr in msg.headers.to.split(',') {
                let addr = addr.trim();
                if !addr.is_empty() {
                    emails.insert(addr.to_string());
                }
            }
        }

        let mut queries = Vec::new();
        match strategy {
            "participants" | "auto" if !emails.is_empty() => {
                let expr = emails
                    .iter()
                    .map(|e| format!("from:{e}"))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                queries.push(expr);
            }
            "subject" => {
                if let Some(first) = source.messages.first() {
                    let subj = strip_reply_prefixes(&first.subject);
                    if !subj.is_empty() && subj != "(no subject)" {
                        queries.push(format!("subject:\"{subj}\""));
                    }
                }
            }
            "tags" if !source.tags.is_empty() => {
                let expr = source
                    .tags
                    .iter()
                    .map(|t| format!("tag:{t}"))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                queries.push(expr);
            }
            _ => {}
        }

        let mut related = Vec::new();
        for q in queries {
            match self.db.search(q.clone(), None, Some(50), None).await {
                Ok(result) => {
                    for th in result.threads {
                        if th.thread_id != params.thread_id
                            && !related
                                .iter()
                                .any(|r: &ThreadSummary| r.thread_id == th.thread_id)
                        {
                            related.push(th);
                            if related.len() >= limit {
                                break;
                            }
                        }
                    }
                }
                Err(_) => continue,
            }
            if related.len() >= limit {
                break;
            }
        }

        // auto fallback to subject if participants yielded < 3 results
        if strategy == "auto" && related.len() < 3 {
            if let Some(first) = source.messages.first() {
                let subj = strip_reply_prefixes(&first.subject);
                if !subj.is_empty() && subj != "(no subject)" {
                    let q = format!("subject:\"{subj}\"");
                    if let Ok(result) = self.db.search(q, None, Some(50), None).await {
                        for th in result.threads {
                            if th.thread_id != params.thread_id
                                && !related
                                    .iter()
                                    .any(|r: &ThreadSummary| r.thread_id == th.thread_id)
                            {
                                related.push(th);
                                if related.len() >= limit {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        let human = format!(
            "Found {} related thread(s) for thread {} using strategy '{}'.",
            related.len(),
            params.thread_id,
            strategy
        );

        let structured = json!({
            "thread_id": params.thread_id,
            "strategy_used": strategy,
            "threads": related,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Get the reply structure of a thread as a chronological tree.
    ///
    /// Use this tool when you need to understand the conversational flow
    /// and reply order within a thread.
    ///
    /// Parameters:
    ///   thread_id    — Thread ID (required)
    ///   body_format  — "text" (default), "markdown", or "html"
    ///
    /// Returns: thread_id, subject, tree[] with message_id, from,
    /// date, subject, depth, children[].
    ///
    /// Tip: Prefer get_thread for reading full thread content. Use this
    /// when reply structure or chronological ordering is important.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn get_conversation_tree(
        &self,
        Parameters(params): Parameters<GetConversationTreeParams>,
    ) -> CallToolResult {
        let _span =
            info_span!("mcp_tool", tool = "get_conversation_tree", thread_id = %params.thread_id);
        let _timer = std::time::Instant::now();

        let tree = match self.db.thread_tree(params.thread_id.clone()).await {
            Ok(t) => t,
            Err(AppError::NotFound(_)) => {
                return crate::mcp::error::not_found_error("Thread", &params.thread_id);
            }
            Err(e) => {
                return crate::mcp::error::error_result(format!(
                    "Failed to fetch conversation tree: {e}"
                ));
            }
        };

        fn count_nodes(node: &crate::api::thread::ConversationNode) -> usize {
            1 + node.children.iter().map(count_nodes).sum::<usize>()
        }

        let total_messages: usize = tree.tree.iter().map(count_nodes).sum();
        let subject = tree.subject.as_deref().unwrap_or("(no subject)");

        let human = format!(
            "Conversation tree for thread '{}' — {} message(s).",
            subject, total_messages
        );

        let structured = json!({
            "thread_id": params.thread_id,
            "subject": subject,
            "tree": tree,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Get the count of threads and messages matching a query without
    /// fetching any results.
    ///
    /// Use this tool to gauge result set size before committing to a full
    /// search. Fast and lightweight — no pagination needed.
    ///
    /// Parameters:
    ///   query — Notmuch query string (required)
    ///
    /// Returns: threads (thread count), messages (message count).
    ///
    /// Strategy: Call count_results first with a broad query, then narrow
    /// and call search when the count is manageable.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn count_results(
        &self,
        Parameters(params): Parameters<CountResultsParams>,
    ) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "count_results", query = %params.query);
        let _timer = std::time::Instant::now();

        if params.query.trim().is_empty() {
            return crate::mcp::error::empty_query_error();
        }

        let (thread_count, message_count) = match self.db.count(params.query.clone()).await {
            Ok(c) => c,
            Err(e) => {
                return crate::mcp::error::error_result(format!("Count query failed: {e}"));
            }
        };

        let human = format!(
            "Query '{}' matches {thread_count} thread(s) containing {message_count} message(s).",
            params.query
        );

        let structured = json!({
            "query": params.query,
            "threads": thread_count,
            "messages": message_count,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }

    /// Get a complete overview of the email archive in a single call.
    ///
    /// This is the best first tool to call when exploring an unfamiliar
    /// archive. Combines statistics, all tags with counts, top senders,
    /// the archive date range, and a sample of recent thread subjects.
    ///
    /// Returns: total_messages, total_threads, tag_count, date_range
    /// { oldest, newest }, tags[], top_senders[], sample_subjects[].
    ///
    /// Strategy table:
    ///   Unfamiliar archive    → Call archive_overview first
    ///   Need specific emails  → Use search with from:/subject:/date:
    ///   Browse by category    → Use list_tags, then tag: searches
    ///   Find key people       → Use list_senders, then from: searches
    ///
    /// Tip: Use the date_range to calibrate date: filters in searches.
    #[rmcp::tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn archive_overview(&self) -> CallToolResult {
        let _span = info_span!("mcp_tool", tool = "archive_overview");
        let _timer = std::time::Instant::now();

        let stats = match self.db.stats().await {
            Ok(s) => s,
            Err(e) => return crate::mcp::error::error_result(format!("Stats failed: {e}")),
        };

        let tags = match self.db.tags().await {
            Ok(t) => t.into_iter().take(100).collect::<Vec<_>>(),
            Err(e) => return crate::mcp::error::error_result(format!("Tags failed: {e}")),
        };

        let senders = match self.db.senders().await {
            Ok(s) => s.into_iter().take(50).collect::<Vec<_>>(),
            Err(e) => return crate::mcp::error::error_result(format!("Senders failed: {e}")),
        };

        let sample_subjects = match self
            .db
            .search("*".into(), Some(0), Some(10), Some("newest".into()))
            .await
        {
            Ok(result) => result
                .threads
                .iter()
                .map(|t| t.subject.clone())
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };

        let human = format!(
            "Archive overview: {} messages, {} threads, {} tag(s), {} sender(s). Sample subjects: {}.",
            stats.total_messages,
            stats.total_threads,
            tags.len(),
            senders.len(),
            sample_subjects.join("; ")
        );

        let structured = json!({
            "total_messages": stats.total_messages,
            "total_threads": stats.total_threads,
            "tag_count": stats.tag_count,
            "date_range": stats.date_range,
            "tags": tags,
            "top_senders": senders,
            "sample_subjects": sample_subjects,
        });

        _span.in_scope(|| trace!(elapsed_ms = _timer.elapsed().as_millis(), "tool completed"));
        crate::mcp::util::tool_success(human, structured)
    }
}
