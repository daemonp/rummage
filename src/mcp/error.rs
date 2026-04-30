//! MCP error response helpers.

use rmcp::model::{CallToolResult, Content};

/// Build a `CallToolResult` with `isError: true` and a single text block.
pub fn error_result(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(message.into())])
}

/// Error response for an empty query.
pub fn empty_query_error() -> CallToolResult {
    error_result(
        "Query is empty. Try: from:someone, tag:inbox, or a keyword. \
         Example: from:alice@example.com, tag:inbox AND date:2013-06..",
    )
}

/// Error response when an entity is not found.
pub fn not_found_error(what: &str, id: &str) -> CallToolResult {
    error_result(format!(
        "{what} not found: {id}. Use search to find valid IDs first."
    ))
}

/// Error response for an unsupported MIME type.
pub fn unsupported_format_error(mime_type: &str) -> CallToolResult {
    error_result(format!(
        "Cannot extract text from {mime_type}. \
         Supported formats: docx, xlsx, pdf, pptx, doc, xls, txt, csv, json, html, md. \
         Use get_attachment for the raw binary."
    ))
}

/// Error response for a missing DB method that is not yet implemented.
pub fn not_implemented_error(feature: &str) -> CallToolResult {
    error_result(format!(
        "{feature} is not yet available. \
         This feature requires backend extensions (Phase 1 of the MCP build plan)."
    ))
}

/// Error response when a search returns zero results, with fuzzy suggestions.
pub fn no_results_error(query: &str, tags: &[String], senders: &[String]) -> CallToolResult {
    let mut suggestions = Vec::new();

    // Tag suggestions
    if query.contains("tag:") {
        let tag_part = query.split("tag:").nth(1).unwrap_or("");
        let tag_query = tag_part.split_whitespace().next().unwrap_or("");
        if !tag_query.is_empty() {
            let close = crate::mcp::util::fuzzy_match(tag_query, tags, 5);
            if !close.is_empty() {
                suggestions.push(format!("Similar tags: {}", close.join(", ")));
            }
        }
    }

    // Sender suggestions
    if query.contains("from:") || query.contains("to:") {
        let prefix = if query.contains("from:") {
            "from:"
        } else {
            "to:"
        };
        let sender_part = query.split(prefix).nth(1).unwrap_or("");
        let sender_query = sender_part.split_whitespace().next().unwrap_or("");
        if !sender_query.is_empty() {
            let close = crate::mcp::util::fuzzy_match(sender_query, senders, 5);
            if !close.is_empty() {
                suggestions.push(format!("Similar senders: {}", close.join(", ")));
            }
        }
    }

    // General suggestions
    suggestions.push("Broaden the date range (e.g., omit date: filters)".to_string());
    suggestions.push("Check spelling of names or terms".to_string());
    suggestions.push("Use tag:* to see available tags, then filter with tag:<name>".to_string());
    suggestions.push("Try a wildcard search with * (e.g., subject:*budget*)".to_string());

    let suggestion_text = if suggestions.is_empty() {
        String::new()
    } else {
        format!("\nDid you mean:\n- {}", suggestions.join("\n- "))
    };

    error_result(format!("No results for '{}'.{}", query, suggestion_text))
}
