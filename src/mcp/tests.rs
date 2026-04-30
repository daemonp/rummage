//! MCP integration tests — protocol compliance and data-layer verification.
//!
//! These tests exercise the `RummageMcpHandler` directly (tools, prompts, get_info)
//! and the underlying `DbHandle` (resources, stats).  They skip gracefully when the
//! test maildir (`mail/test-archive`) is absent.

use crate::api::thread::{MessageDetail, MessageHeaders};
use crate::db;
use crate::mail::body::{html_to_markdown, html_to_text};
use crate::mcp::prompts::{AnalyzeCorrespondenceArgs, FindEmailsAboutArgs, SummarizeThreadArgs};
use crate::mcp::tools::{
    CountResultsParams, FindRelatedThreadsParams, GetAttachmentTextParams,
    GetConversationTreeParams, GetMessageParams, GetThreadParams, ListSendersParams,
    ListTagsParams, SearchParams, SearchWithinThreadParams,
};
use crate::mcp::{
    build_instructions, extract_snippet, format_body, resources, BodyFormat, RummageMcpHandler,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::handler::server::ServerHandler;

// ── Shared helpers ─────────────────────────────────────────────────

fn test_maildir() -> Option<std::path::PathBuf> {
    let maildir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("mail/test-archive");
    if maildir.exists() {
        Some(maildir)
    } else {
        None
    }
}

async fn setup_handler() -> Option<RummageMcpHandler> {
    let maildir = test_maildir()?;
    let db = db::spawn_database_worker(&maildir, None, false)
        .await
        .ok()?;
    Some(RummageMcpHandler::new(db).await)
}

/// Extract the JSON text block from a `CallToolResult` (the second content item).
fn tool_json(result: &rmcp::model::CallToolResult) -> Option<serde_json::Value> {
    result.content.iter().find_map(|c| {
        c.as_text().and_then(|t| {
            let s = t.text.trim();
            if s.starts_with('{') || s.starts_with('[') {
                serde_json::from_str(s).ok()
            } else {
                None
            }
        })
    })
}

/// Extract the human-readable text block from a `CallToolResult`.
fn tool_text(result: &rmcp::model::CallToolResult) -> Option<String> {
    result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
}

// ── Server info / capabilities ─────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mcp_server_info() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let info = handler.get_info();

    assert_eq!(
        info.server_info.name, "rummage",
        "server name should be rummage"
    );
    assert!(
        info.capabilities.tools.is_some(),
        "tools capability should be enabled"
    );
    assert!(
        info.capabilities.resources.is_some(),
        "resources capability should be enabled"
    );
    assert!(
        info.capabilities.prompts.is_some(),
        "prompts capability should be enabled"
    );

    let instructions = info.instructions.as_deref().unwrap_or("");
    assert!(!instructions.is_empty(), "instructions should not be empty");
    assert!(
        instructions.contains("email archive"),
        "instructions should mention archive: {}",
        instructions
    );
}

// ── Tools list ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_tools_list() {
    let Some(handler) = setup_handler().await else {
        return;
    };

    let expected = [
        "search",
        "get_thread",
        "get_message",
        "get_attachment",
        "get_attachment_text",
        "list_tags",
        "list_senders",
        "get_stats",
        "search_within_thread",
        "find_related_threads",
        "get_conversation_tree",
        "count_results",
        "archive_overview",
    ];

    for name in expected {
        let tool = handler
            .get_tool(name)
            .unwrap_or_else(|| panic!("tool {} should be registered", name));
        let desc = tool.description.as_deref().unwrap_or("");
        assert!(
            desc.len() > 50,
            "tool {} description too short ({} chars): {}",
            name,
            desc.len(),
            desc
        );
    }
}

// ── Search tool ────────────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_search_inbox() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler
        .search(Parameters(SearchParams {
            query: "tag:inbox".into(),
            limit: None,
            offset: None,
            sort: None,
        }))
        .await;

    assert_eq!(result.is_error, Some(false), "search should succeed");
    let data = tool_json(&result).expect("structured JSON content");
    assert!(
        data["total_results"].as_u64().unwrap_or(0) > 0,
        "inbox should have results"
    );
    assert!(
        !data["threads"].as_array().unwrap().is_empty(),
        "threads array should not be empty"
    );
}

#[tokio::test]
async fn test_mcp_search_pagination() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler
        .search(Parameters(SearchParams {
            query: "tag:inbox".into(),
            limit: Some(5),
            offset: Some(0),
            sort: None,
        }))
        .await;

    assert_eq!(
        result.is_error,
        Some(false),
        "paginated search should succeed"
    );
    let data = tool_json(&result).expect("structured JSON content");
    let threads = data["threads"].as_array().unwrap();
    assert!(
        threads.len() <= 5,
        "should return at most 5 threads, got {}",
        threads.len()
    );
}

#[tokio::test]
async fn test_mcp_search_empty_query() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler
        .search(Parameters(SearchParams {
            query: "".into(),
            limit: None,
            offset: None,
            sort: None,
        }))
        .await;

    assert_eq!(
        result.is_error,
        Some(true),
        "empty query should return an error"
    );
}

#[tokio::test]
async fn test_mcp_search_no_results() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler
        .search(Parameters(SearchParams {
            query: "tag:nonexistent-tag-xyz123".into(),
            limit: None,
            offset: None,
            sort: None,
        }))
        .await;

    assert_eq!(
        result.is_error,
        Some(true),
        "nonsensical query should return an error-like result with help"
    );
    let text = tool_text(&result).expect("text content");
    assert!(
        text.contains("No results") || text.contains("Suggestions"),
        "should provide a helpful message: {}",
        text
    );
}

// ── Get thread tool ────────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_get_thread() {
    let Some(handler) = setup_handler().await else {
        return;
    };

    // Fetch a thread ID via search.
    let search_result = handler
        .search(Parameters(SearchParams {
            query: "tag:inbox".into(),
            limit: Some(1),
            offset: Some(0),
            sort: None,
        }))
        .await;
    let search_json = tool_json(&search_result).expect("search JSON");
    let threads = search_json["threads"].as_array().unwrap();
    if threads.is_empty() {
        return;
    }
    let thread_id = threads[0]["thread_id"].as_str().unwrap();

    let result = handler
        .get_thread(Parameters(GetThreadParams {
            thread_id: thread_id.into(),
            body_format: Default::default(),
        }))
        .await;

    assert_eq!(result.is_error, Some(false), "get_thread should succeed");
    let data = tool_json(&result).expect("thread JSON");
    let messages = data["messages"].as_array().unwrap();
    assert!(!messages.is_empty(), "thread should contain messages");
}

#[tokio::test]
async fn test_mcp_get_thread_not_found() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler
        .get_thread(Parameters(GetThreadParams {
            thread_id: "nonexistent-thread-id-12345".into(),
            body_format: Default::default(),
        }))
        .await;

    assert_eq!(
        result.is_error,
        Some(true),
        "fake thread id should return error"
    );
}

// ── Get stats tool ─────────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_get_stats() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler.get_stats().await;

    assert_eq!(result.is_error, Some(false), "get_stats should succeed");
    let data = tool_json(&result).expect("stats JSON");
    assert!(
        data["total_messages"].as_u64().unwrap_or(0) > 0,
        "total_messages should be > 0"
    );
    assert!(
        data["total_threads"].as_u64().unwrap_or(0) > 0,
        "total_threads should be > 0"
    );
    assert!(
        data["date_range"].is_object() || data["date_range"].is_null(),
        "date_range should be present"
    );
}

// ── Archive overview tool ──────────────────────────────────────────

#[tokio::test]
async fn test_mcp_archive_overview() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler.archive_overview().await;

    assert_eq!(
        result.is_error,
        Some(false),
        "archive_overview should succeed"
    );
    let data = tool_json(&result).expect("overview JSON");
    assert!(
        data["total_messages"].as_u64().unwrap_or(0) > 0,
        "overview should include total_messages"
    );
    assert!(
        data["total_threads"].as_u64().unwrap_or(0) > 0,
        "overview should include total_threads"
    );
    assert!(
        data["tags"].is_array(),
        "overview should include tags array"
    );
    assert!(
        data["top_senders"].is_array(),
        "overview should include top_senders array"
    );
    assert!(
        data["sample_subjects"].is_array(),
        "overview should include sample_subjects array"
    );
}

// ── List tags tool ─────────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_list_tags() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler
        .list_tags(Parameters(ListTagsParams { limit: None }))
        .await;

    assert_eq!(result.is_error, Some(false), "list_tags should succeed");
    let data = tool_json(&result).expect("tags JSON");
    let tags = data["tags"].as_array().unwrap();
    assert!(!tags.is_empty(), "tags array should not be empty");
}

// ── Count results tool ─────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_count_results() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler
        .count_results(Parameters(CountResultsParams {
            query: "tag:inbox".into(),
        }))
        .await;

    assert_eq!(result.is_error, Some(false), "count_results should succeed");
    let data = tool_json(&result).expect("count JSON");
    let threads = data["threads"].as_u64().unwrap_or(0);
    let messages = data["messages"].as_u64().unwrap_or(0);
    assert!(
        threads > 0 || messages > 0,
        "count should return non-zero thread or message count"
    );
}

// ── Get message tool ──────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_get_message() {
    let Some(handler) = setup_handler().await else {
        return;
    };

    // Fetch a thread to get a message ID.
    let search_result = handler
        .search(Parameters(SearchParams {
            query: "tag:inbox".into(),
            limit: Some(1),
            offset: Some(0),
            sort: None,
        }))
        .await;
    let search_json = tool_json(&search_result).expect("search JSON");
    let threads = search_json["threads"].as_array().unwrap();
    if threads.is_empty() {
        return;
    }
    let thread_id = threads[0]["thread_id"].as_str().unwrap();

    let thread_result = handler
        .get_thread(Parameters(GetThreadParams {
            thread_id: thread_id.into(),
            body_format: Default::default(),
        }))
        .await;
    let thread_json = tool_json(&thread_result).expect("thread JSON");
    let messages = thread_json["messages"].as_array().unwrap();
    if messages.is_empty() {
        return;
    }
    let msg_id = messages[0]["message_id"].as_str().unwrap();

    let result = handler
        .get_message(Parameters(GetMessageParams {
            message_id: msg_id.into(),
            body_format: Default::default(),
        }))
        .await;

    assert_eq!(result.is_error, Some(false), "get_message should succeed");
    let data = tool_json(&result).expect("message JSON");
    assert!(
        data["message_id"].as_str().is_some(),
        "should have message_id"
    );
    assert!(data["from"].as_str().is_some(), "should have from");
    assert!(data["subject"].as_str().is_some(), "should have subject");
    assert!(data["body"].as_str().is_some(), "should have body");
}

// ── List senders tool ──────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_list_senders() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler
        .list_senders(Parameters(ListSendersParams {
            limit: Some(5),
            query: None,
        }))
        .await;

    assert_eq!(result.is_error, Some(false), "list_senders should succeed");
    let data = tool_json(&result).expect("senders JSON");
    let senders = data["senders"].as_array().unwrap();
    assert!(!senders.is_empty(), "senders array should not be empty");
    assert!(
        senders.len() <= 5,
        "should respect limit, got {}",
        senders.len()
    );
}

#[tokio::test]
async fn test_mcp_list_senders_with_query() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = handler
        .list_senders(Parameters(ListSendersParams {
            limit: Some(10),
            query: Some("tag:inbox".into()),
        }))
        .await;

    assert_eq!(
        result.is_error,
        Some(false),
        "list_senders with query should succeed"
    );
}

// ── Search within thread tool ──────────────────────────────────────

#[tokio::test]
async fn test_mcp_search_within_thread() {
    let Some(handler) = setup_handler().await else {
        return;
    };

    // Get a thread to search within.
    let search_result = handler
        .search(Parameters(SearchParams {
            query: "tag:inbox".into(),
            limit: Some(1),
            offset: Some(0),
            sort: None,
        }))
        .await;
    let search_json = tool_json(&search_result).expect("search JSON");
    let threads = search_json["threads"].as_array().unwrap();
    if threads.is_empty() {
        return;
    }
    let thread_id = threads[0]["thread_id"].as_str().unwrap();
    // Use a common word likely to appear in any email
    let result = handler
        .search_within_thread(Parameters(SearchWithinThreadParams {
            thread_id: thread_id.into(),
            term: "the".into(),
            body_format: Default::default(),
        }))
        .await;

    assert_eq!(
        result.is_error,
        Some(false),
        "search_within_thread should succeed"
    );
    let data = tool_json(&result).expect("search_within_thread JSON");
    assert!(
        data["total_in_thread"].as_u64().unwrap_or(0) > 0,
        "thread should have messages"
    );
}

// ── Find related threads tool ──────────────────────────────────────

#[tokio::test]
async fn test_mcp_find_related_threads() {
    let Some(handler) = setup_handler().await else {
        return;
    };

    let search_result = handler
        .search(Parameters(SearchParams {
            query: "tag:inbox".into(),
            limit: Some(1),
            offset: Some(0),
            sort: None,
        }))
        .await;
    let search_json = tool_json(&search_result).expect("search JSON");
    let threads = search_json["threads"].as_array().unwrap();
    if threads.is_empty() {
        return;
    }
    let thread_id = threads[0]["thread_id"].as_str().unwrap();

    let result = handler
        .find_related_threads(Parameters(FindRelatedThreadsParams {
            thread_id: thread_id.into(),
            strategy: Some("auto".into()),
            limit: Some(5),
        }))
        .await;

    assert_eq!(
        result.is_error,
        Some(false),
        "find_related_threads should succeed"
    );
    let data = tool_json(&result).expect("find_related JSON");
    assert!(data["threads"].is_array(), "should return threads array");
    assert!(
        data["strategy_used"].as_str().is_some(),
        "should report strategy used"
    );
}

// ── Get conversation tree tool ─────────────────────────────────────

#[tokio::test]
async fn test_mcp_get_conversation_tree() {
    let Some(handler) = setup_handler().await else {
        return;
    };

    let search_result = handler
        .search(Parameters(SearchParams {
            query: "tag:inbox".into(),
            limit: Some(1),
            offset: Some(0),
            sort: None,
        }))
        .await;
    let search_json = tool_json(&search_result).expect("search JSON");
    let threads = search_json["threads"].as_array().unwrap();
    if threads.is_empty() {
        return;
    }
    let thread_id = threads[0]["thread_id"].as_str().unwrap();

    let result = handler
        .get_conversation_tree(Parameters(GetConversationTreeParams {
            thread_id: thread_id.into(),
            body_format: Default::default(),
        }))
        .await;

    assert_eq!(
        result.is_error,
        Some(false),
        "get_conversation_tree should succeed"
    );
    let data = tool_json(&result).expect("conversation tree JSON");
    assert!(
        data["thread_id"].as_str().is_some(),
        "should have thread_id"
    );
    assert!(
        data["tree"].is_object() || data["tree"].is_array(),
        "should have tree"
    );
}

// ── Resources ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_list_resources() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = resources::list_resources(&handler.db).await;

    let uris: Vec<_> = result.resources.iter().map(|r| r.uri.as_str()).collect();
    assert!(
        uris.contains(&"rummage://tags"),
        "should list rummage://tags resource"
    );
    assert!(
        uris.contains(&"rummage://stats"),
        "should list rummage://stats resource"
    );
}

#[tokio::test]
async fn test_mcp_read_resource_stats() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let result = resources::read_resource(&handler.db, "rummage://stats").await;

    assert!(result.is_ok(), "reading stats resource should succeed");
    let contents = result.unwrap().contents;
    assert!(
        !contents.is_empty(),
        "resource contents should not be empty"
    );

    let text = match &contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.clone(),
        _ => panic!("expected text resource contents"),
    };

    let data: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert!(
        data["total_messages"].as_u64().unwrap_or(0) > 0,
        "stats resource should contain total_messages"
    );
}

// ── Prompts ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_list_prompts() {
    let Some(handler) = setup_handler().await else {
        return;
    };

    // Verify all 4 prompt methods exist and return non-empty results.
    let guide = handler.search_guide().await;
    assert!(!guide.is_empty(), "search_guide should return messages");

    let find = handler
        .find_emails_about(Parameters(FindEmailsAboutArgs {
            description: "budget discussions".into(),
        }))
        .await;
    assert!(!find.is_empty(), "find_emails_about should return messages");

    let analyze = handler
        .analyze_correspondence(Parameters(AnalyzeCorrespondenceArgs {
            email: "alice@example.com".into(),
        }))
        .await;
    assert!(
        !analyze.is_empty(),
        "analyze_correspondence should return messages"
    );

    // summarize_thread needs a real thread ID.
    let search_result = handler
        .search(Parameters(SearchParams {
            query: "tag:inbox".into(),
            limit: Some(1),
            offset: Some(0),
            sort: None,
        }))
        .await;
    let search_json = tool_json(&search_result).expect("search JSON");
    let threads = search_json["threads"].as_array().unwrap();
    if !threads.is_empty() {
        let thread_id = threads[0]["thread_id"].as_str().unwrap();
        let summary = handler
            .summarize_thread(Parameters(SummarizeThreadArgs {
                thread_id: thread_id.into(),
            }))
            .await;
        assert!(summary.is_ok(), "summarize_thread should succeed");
        assert!(
            !summary.unwrap().is_empty(),
            "summarize_thread should return messages"
        );
    }
}

#[tokio::test]
async fn test_mcp_get_search_guide_prompt() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let guide = handler.search_guide().await;

    assert!(
        !guide.is_empty(),
        "search_guide should return at least one message"
    );
    let text = match &guide[0].content {
        rmcp::model::PromptMessageContent::Text { text } => text.clone(),
        _ => panic!("expected text prompt content"),
    };
    assert!(
        text.contains("notmuch query syntax"),
        "search guide should mention notmuch syntax: {}",
        text
    );
    assert!(
        text.contains("from:"),
        "search guide should mention from: filter"
    );
    assert!(
        text.contains("tag:"),
        "search guide should mention tag: filter"
    );
}

// ── Helper function tests ──────────────────────────────────────────

#[test]
fn test_html_to_text_basic() {
    let html = "<p>Hello <b>world</b></p>";
    let text = html_to_text(html);
    assert!(text.contains("Hello"));
    assert!(text.contains("world"));
    assert!(!text.contains("<p>"));
    assert!(!text.contains("<b>"));
}

#[test]
fn test_html_to_text_entities() {
    let html = "&lt;tag&gt; &amp; &quot;quote&quot;";
    let text = html_to_text(html);
    assert!(text.contains("<tag>"));
    assert!(text.contains("&"));
    assert!(text.contains("\"quote\""));
}

#[test]
fn test_html_to_markdown_basic() {
    let html = "<h1>Title</h1><p>Body</p>";
    let md = html_to_markdown(html);
    assert!(md.contains("# Title"));
    assert!(md.contains("Body"));
}

#[test]
fn test_html_to_markdown_formatting() {
    let html = "<b>bold</b> <i>italic</i> <strong>strong</strong> <em>em</em>";
    let md = html_to_markdown(html);
    assert!(md.contains("**bold**"));
    assert!(md.contains("*italic*"));
    assert!(md.contains("**strong**"));
    assert!(md.contains("*em*"));
}

#[test]
fn test_format_body_html() {
    let msg = MessageDetail {
        message_id: "test".into(),
        headers: MessageHeaders {
            from: "a@b".into(),
            to: "c@d".into(),
            cc: None,
            bcc: None,
        },
        date: 0,
        date_relative: "now".into(),
        subject: "Test".into(),
        content: "<p>Hello</p>".into(),
        content_type: "text/html".into(),
        attachments: vec![],
        body_text: None,
        body_markdown: None,
        tags: vec![],
        in_reply_to: None,
        references: vec![],
    };
    assert_eq!(format_body(&msg, &BodyFormat::Html), "<p>Hello</p>");
    assert!(format_body(&msg, &BodyFormat::Text).contains("Hello"));
    assert!(format_body(&msg, &BodyFormat::Markdown).contains("Hello"));
}

#[test]
fn test_extract_snippet_found() {
    let text = "The quick brown fox jumps over the lazy dog";
    let snippet = extract_snippet(text, "fox");
    assert!(snippet.contains("fox"));
    assert!(snippet.contains("quick"));
    assert!(snippet.contains("jumps"));
}

#[test]
fn test_extract_snippet_not_found() {
    let text = "The quick brown fox";
    let snippet = extract_snippet(text, "cat");
    assert_eq!(snippet, "The quick brown fox");
}

#[test]
fn test_extract_snippet_multibyte() {
    let text = "Hello 世界 this is a test string for multibyte characters";
    let snippet = extract_snippet(text, "世界");
    assert!(snippet.contains("世界"));
    assert!(snippet.contains("Hello"));
}

#[tokio::test]
async fn test_build_instructions() {
    let Some(handler) = setup_handler().await else {
        return;
    };
    let instructions = build_instructions(&handler.db).await;
    assert!(
        instructions.contains("email archive"),
        "instructions should mention archive"
    );
    assert!(
        instructions.contains("Search uses notmuch query syntax"),
        "instructions should mention query syntax"
    );
}

// ── Attachment text / batdoc-core integration ──────────────────────

#[tokio::test]
async fn test_mcp_get_attachment_text() {
    let Some(handler) = setup_handler().await else {
        return;
    };

    // Search for threads with attachments
    let search_result = handler
        .search(Parameters(SearchParams {
            query: "has:attachment".into(),
            limit: Some(20),
            offset: Some(0),
            sort: None,
        }))
        .await;

    let data = tool_json(&search_result).expect("search JSON");
    let threads = data["threads"].as_array().unwrap();

    // Find a thread with a document attachment (not just image)
    let mut found = None;
    for thread in threads {
        let thread_id = thread["thread_id"].as_str().unwrap();
        let thread_detail = handler
            .get_thread(Parameters(GetThreadParams {
                thread_id: thread_id.into(),
                body_format: Default::default(),
            }))
            .await;
        let detail_json = tool_json(&thread_detail).expect("thread JSON");
        let messages = detail_json["messages"].as_array().unwrap();

        let empty_attachments = vec![];
        for msg in messages {
            let attachments = msg["attachments"].as_array().unwrap_or(&empty_attachments);
            for att in attachments {
                let ct = att["content_type"].as_str().unwrap_or("");
                let fname = att["filename"].as_str().unwrap_or("");
                let part = att["part"].as_u64().unwrap_or(0) as usize;
                let msg_id = msg["message_id"].as_str().unwrap();

                if ct == "application/pdf"
                    || fname.ends_with(".pdf")
                    || fname.ends_with(".docx")
                    || fname.ends_with(".xlsx")
                    || fname.ends_with(".doc")
                    || fname.ends_with(".xls")
                    || fname.ends_with(".pptx")
                {
                    found = Some((msg_id.to_string(), part));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }

    let Some((msg_id, part)) = found else {
        println!("No document attachments found in test maildir, skipping batdoc test");
        return;
    };

    let result = handler
        .get_attachment_text(Parameters(GetAttachmentTextParams {
            message_id: msg_id,
            part,
            format: Some("text".into()),
        }))
        .await;

    assert_eq!(
        result.is_error,
        Some(false),
        "attachment_text should succeed for document: {:?}",
        tool_text(&result)
    );
    let data = tool_json(&result).expect("attachment text JSON");
    let content = data["content"].as_str().unwrap_or("");
    assert!(!content.is_empty(), "extracted text should not be empty");
    assert!(
        content.len() > 10,
        "extracted text should have meaningful content, got: {}",
        content
    );
}
