//! Utility helpers for MCP tool output formatting and truncation.

use crate::api::search::ThreadSummary;
use crate::api::thread::MessageDetail;
use rmcp::model::{CallToolResult, Content};

pub const MCP_MAX_BODY_CHARS: usize = 4000;
pub const MCP_MAX_ATTACHMENT_BYTES: usize = 50_000;

/// Truncate a message body for MCP output, adding a pointer to full content.
#[must_use]
pub fn truncate_body(text: &str, max_chars: usize) -> String {
    crate::mail::body::truncate_text_with_suffix(
        text,
        max_chars,
        " [truncated, use get_message for full content]",
    )
}

/// Truncate attachment extracted text.
#[must_use]
pub fn truncate_attachment_text(text: &str, max_bytes: usize) -> String {
    crate::mail::body::truncate_text_with_suffix(
        text,
        max_bytes,
        " [truncated, document continues...]",
    )
}

/// Truncate a message body using the standard MCP 4000-char limit.
#[must_use]
pub fn truncate_thread_body(text: &str) -> String {
    truncate_body(text, MCP_MAX_BODY_CHARS)
}

/// Truncate attachment text using the standard MCP 50 KB limit.
#[must_use]
pub fn limit_attachment_text(text: &str) -> String {
    truncate_attachment_text(text, MCP_MAX_ATTACHMENT_BYTES)
}

/// Format a message body in the requested format and truncate to the standard limit.
#[must_use]
pub fn format_and_truncate_body(
    msg: &MessageDetail,
    body_format: &crate::mcp::BodyFormat,
) -> String {
    let body = crate::mcp::format_body(msg, body_format);
    truncate_thread_body(&body)
}

/// Extract a snippet of text around a search match.
#[must_use]
pub fn snippet_around_match(text: &str, query: &str, context_chars: usize) -> Option<String> {
    let text_lower = text.to_lowercase();
    let query_lower = query.to_lowercase();
    let pos = text_lower.find(&query_lower)?;

    let match_start = pos;
    let match_end = pos + query.len();

    let snippet_start = match_start.saturating_sub(context_chars);
    let snippet_end = (match_end + context_chars).min(text.len());

    // Adjust to char boundaries.
    let mut start = snippet_start;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = snippet_end;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    let prefix = if start > 0 { "..." } else { "" };
    let suffix = if end < text.len() { "..." } else { "" };

    Some(format!("{}{}{}", prefix, &text[start..end], suffix))
}

/// Format an error message with helpful suggestions for the LLM.
#[must_use]
pub fn format_search_error(query: &str) -> String {
    format!(
        "No results for '{}'. Suggestions:\n\
         - Broaden the date range (e.g., omit date: filters)\n\
         - Check spelling of names or terms\n\
         - Use tag:* to see available tags, then filter with tag:<name>\n\
         - Try a wildcard search with * (e.g., subject:*budget*)\n\
         - Use from: or to: with partial email addresses",
        query
    )
}

/// Build a successful tool result with human-readable text + structured JSON.
pub fn tool_success(human: impl Into<String>, structured: serde_json::Value) -> CallToolResult {
    CallToolResult::success(vec![
        Content::text(human.into()),
        Content::json(structured).unwrap_or_else(|_| Content::text("{}".to_string())),
    ])
}

/// Cap search results to a maximum number of threads.
#[must_use]
pub fn cap_search_results(threads: Vec<ThreadSummary>) -> Vec<ThreadSummary> {
    threads.into_iter().take(100).collect()
}

/// Simple fuzzy string matching using substring containment and Levenshtein distance.
#[must_use]
pub fn fuzzy_match(query: &str, candidates: &[String], max_results: usize) -> Vec<String> {
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(usize, String)> = candidates
        .iter()
        .map(|c| {
            let c_lower = c.to_lowercase();
            let score = if c_lower.contains(&query_lower) || query_lower.contains(&c_lower) {
                0
            } else {
                levenshtein_distance(&query_lower, &c_lower)
            };
            (score, c.clone())
        })
        .collect();
    scored.sort_by_key(|a| a.0);
    scored
        .into_iter()
        .take(max_results)
        .map(|(_, s)| s)
        .collect()
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev = vec![0usize; b_len + 1];
    let mut curr = vec![0usize; b_len + 1];

    for (j, prev_j) in prev.iter_mut().enumerate().take(b_len + 1) {
        *prev_j = j;
    }

    for (i, a_ch) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, b_ch) in b_chars.iter().enumerate() {
            let cost = if a_ch == b_ch { 0 } else { 1 };
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_body_short() {
        let text = "Short message";
        assert_eq!(truncate_body(text, 100), "Short message");
    }

    #[test]
    fn test_truncate_body_long() {
        let text = "This is a very long message body that needs to be truncated for MCP output";
        let result = truncate_body(text, 20);
        assert!(result.contains("[truncated, use get_message for full content]"));
        assert!(result.starts_with("This is a very"));
    }

    #[test]
    fn test_truncate_attachment_text_short() {
        let text = "Short doc";
        assert_eq!(truncate_attachment_text(text, 100), "Short doc");
    }

    #[test]
    fn test_truncate_attachment_text_long() {
        let text = "This is a very long attachment text that needs truncation";
        let result = truncate_attachment_text(text, 20);
        assert!(result.contains("[truncated, document continues...]"));
    }

    #[test]
    fn test_snippet_around_match_found() {
        let text = "The quick brown fox jumps over the lazy dog";
        let result = snippet_around_match(text, "fox", 6).unwrap();
        assert!(result.contains("fox"));
        assert!(result.contains("brown"));
        assert!(result.contains("jumps"));
    }

    #[test]
    fn test_snippet_around_match_not_found() {
        let text = "The quick brown fox";
        assert!(snippet_around_match(text, "cat", 5).is_none());
    }

    #[test]
    fn test_snippet_around_match_multiple() {
        // Should return first match.
        let text = "cat dog cat bird";
        let result = snippet_around_match(text, "cat", 4).unwrap();
        assert!(result.contains("cat"));
        assert!(result.contains("dog"));
    }

    #[test]
    fn test_snippet_around_match_context_edges() {
        let text = "abcdefgh";
        let result = snippet_around_match(text, "cde", 2).unwrap();
        assert_eq!(result, "abcdefg...");
    }

    #[test]
    fn test_snippet_around_match_case_insensitive() {
        let text = "The Quick Brown Fox";
        let result = snippet_around_match(text, "fox", 3).unwrap();
        assert!(result.contains("Fox"));
    }

    #[test]
    fn test_format_search_error() {
        let msg = format_search_error("subject: budget 2024");
        assert!(msg.contains("No results for 'subject: budget 2024'"));
        assert!(msg.contains("tag:*"));
        assert!(msg.contains("Broaden"));
    }

    #[test]
    fn test_cap_search_results() {
        let results: Vec<ThreadSummary> = (0..150)
            .map(|i| ThreadSummary {
                thread_id: i.to_string(),
                subject: String::new(),
                authors: String::new(),
                matched_messages: 0,
                total_messages: 0,
                newest_date: 0,
                oldest_date: 0,
                tags: Vec::new(),
                preview: None,
                has_attachments: false,
            })
            .collect();
        let capped = cap_search_results(results);
        assert_eq!(capped.len(), 100);
    }

    #[test]
    fn test_fuzzy_match_exact() {
        let candidates = vec!["inbox".to_string(), "sent".to_string(), "spam".to_string()];
        let result = fuzzy_match("inbox", &candidates, 1);
        assert_eq!(result, vec!["inbox"]);
    }

    #[test]
    fn test_fuzzy_match_close() {
        let candidates = vec![
            "inbox".to_string(),
            "sent".to_string(),
            "drafts".to_string(),
        ];
        let result = fuzzy_match("inbix", &candidates, 3);
        assert_eq!(result[0], "inbox");
    }

    #[test]
    fn test_fuzzy_match_substring() {
        let candidates = vec!["important".to_string(), "work".to_string()];
        let result = fuzzy_match("port", &candidates, 3);
        assert_eq!(result[0], "important");
    }
}
