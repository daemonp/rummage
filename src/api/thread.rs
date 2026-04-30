use crate::db::DbHandle;
use crate::error::{AppError, Result};
use crate::mail;
use axum::extract::{Path, Query, State};
use axum::Json;
use notmuch::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct ThreadParams {
    pub m: Option<String>, // match IDs, comma separated
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadDetail {
    pub thread_id: String,
    pub tags: Vec<String>,
    pub messages: Vec<MessageDetail>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageDetail {
    pub message_id: String,
    pub headers: MessageHeaders,
    pub date: i64,
    pub date_relative: String,
    pub subject: String,
    pub content: String,
    pub content_type: String,
    pub attachments: Vec<AttachmentSummary>,
    pub body_text: Option<String>,
    pub body_markdown: Option<String>,
    pub tags: Vec<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageHeaders {
    pub from: String,
    pub to: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachmentSummary {
    pub filename: Option<String>,
    pub content_type: String,
    pub part: usize,
    pub size_bytes: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationNode {
    pub message_id: String,
    pub from: String,
    pub date: String,
    pub subject: String,
    pub depth: usize,
    pub children: Vec<ConversationNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationTree {
    pub thread_id: String,
    pub subject: Option<String>,
    pub tags: Vec<String>,
    pub tree: Vec<ConversationNode>,
}

/// Retrieve all messages in a thread by thread ID.
///
/// # Errors
/// Returns `AppError::Notmuch` on database failures, or `AppError::Internal`
/// if the DB worker channel closes unexpectedly.
pub async fn handler(
    State(db): State<DbHandle>,
    Path(thread_id): Path<String>,
    Query(_params): Query<ThreadParams>,
) -> Result<Json<ThreadDetail>> {
    let result = db.thread(thread_id).await?;
    Ok(Json(result))
}

/// Synchronous thread retrieval against an open notmuch `Database`.
///
/// Uses a single query: `search_threads()` to get both tags and messages
/// from the thread object directly, avoiding the previous double-query.
///
/// # Errors
/// Returns `AppError::Notmuch` on query or iteration failures,
/// or `AppError::MailParse` / `AppError::Io` if a message file is unreadable.
pub fn do_thread(db: &Database, thread_id: &str) -> Result<ThreadDetail> {
    let query = db
        .create_query(&format!("thread:{thread_id}"))
        .map_err(AppError::Notmuch)?;

    let threads = query.search_threads().map_err(AppError::Notmuch)?;
    let thread = threads
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound(format!("thread not found: {thread_id}")))?;

    let tags: Vec<String> = thread.tags().collect();

    // Walk messages from the thread object — no second query needed.
    let messages_iter = thread.messages();
    let mut details = Vec::new();
    for msg in messages_iter {
        let mut detail = mail::extract_message(&msg)?;

        // Override threading headers from notmuch (authoritative source).
        if let Ok(Some(irt)) = msg.header("In-Reply-To") {
            detail.in_reply_to = Some(irt.trim().to_string());
        }
        if let Ok(Some(refs)) = msg.header("References") {
            detail.references = refs
                .split_whitespace()
                .map(|s| s.trim().to_string())
                .collect();
        }

        details.push(detail);
    }

    Ok(ThreadDetail {
        thread_id: thread_id.to_string(),
        tags,
        messages: details,
    })
}

// ── Thread tree (conversation structure) ──────────────────────────

#[derive(Debug)]
struct TreeMsg {
    message_id: String,
    from: String,
    date: String,
    subject: String,
    in_reply_to: Option<String>,
    references: Vec<String>,
    date_timestamp: i64,
}

fn strip_angle_brackets(s: &str) -> String {
    s.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

/// Build the reply-tree structure for a thread.
///
/// # Errors
/// Returns `AppError::Notmuch` on query failures,
/// or `AppError::NotFound` if the thread does not exist.
pub fn do_thread_tree(db: &Database, thread_id: &str) -> Result<ConversationTree> {
    let query = db
        .create_query(&format!("thread:{thread_id}"))
        .map_err(AppError::Notmuch)?;
    let threads = query.search_threads().map_err(AppError::Notmuch)?;
    let thread = threads
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound(format!("thread not found: {thread_id}")))?;

    let tags: Vec<String> = thread.tags().collect();

    let mut nodes = Vec::new();
    for msg in thread.messages() {
        let message_id = msg.id().to_string();
        let date_timestamp = msg.date();
        let date = chrono::DateTime::from_timestamp(date_timestamp, 0).map_or_else(
            || "unknown".into(),
            |dt| dt.format("%Y-%m-%d %H:%M").to_string(),
        );

        let from = msg
            .header("From")
            .ok()
            .flatten()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let subject = msg
            .header("Subject")
            .ok()
            .flatten()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "(no subject)".to_string());

        let in_reply_to = msg
            .header("In-Reply-To")
            .ok()
            .flatten()
            .and_then(|s| s.split_whitespace().next().map(strip_angle_brackets));

        let references = msg
            .header("References")
            .ok()
            .flatten()
            .map(|s| {
                s.split_whitespace()
                    .map(strip_angle_brackets)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        nodes.push(TreeMsg {
            message_id,
            from,
            date,
            subject,
            in_reply_to,
            references,
            date_timestamp,
        });
    }

    let id_to_idx: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.message_id.clone(), i))
        .collect();

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut roots = Vec::new();

    let min_timestamp = nodes.iter().map(|n| n.date_timestamp).min().unwrap_or(0);

    for (idx, node) in nodes.iter().enumerate() {
        let parent = node
            .in_reply_to
            .as_ref()
            .and_then(|irt| id_to_idx.get(irt).copied())
            .or_else(|| {
                node.references
                    .iter()
                    .rev()
                    .find_map(|r| id_to_idx.get(r).copied())
            });

        let is_root = match parent {
            None => true,
            Some(p) if p == idx => true,
            Some(_) => node.date_timestamp == min_timestamp,
        };

        if is_root {
            roots.push(idx);
        } else {
            children[parent.unwrap()].push(idx);
        }
    }

    roots.sort_by_key(|&i| nodes[i].date_timestamp);
    for child_list in &mut children {
        child_list.sort_by_key(|&i| nodes[i].date_timestamp);
    }

    fn build_node(
        idx: usize,
        depth: usize,
        nodes: &[TreeMsg],
        children: &[Vec<usize>],
    ) -> ConversationNode {
        let node = &nodes[idx];
        ConversationNode {
            message_id: node.message_id.clone(),
            from: node.from.clone(),
            date: node.date.clone(),
            subject: node.subject.clone(),
            depth,
            children: children[idx]
                .iter()
                .map(|&c| build_node(c, depth + 1, nodes, children))
                .collect(),
        }
    }

    let tree = roots
        .iter()
        .map(|&i| build_node(i, 0, &nodes, &children))
        .collect::<Vec<_>>();

    let subject = tree.first().map(|n| n.subject.clone());

    Ok(ConversationTree {
        thread_id: thread_id.to_string(),
        subject,
        tags,
        tree,
    })
}
