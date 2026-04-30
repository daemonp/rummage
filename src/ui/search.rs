//! Search page — sidebar + results list + reader pane.

use dioxus::prelude::*;

use crate::api::search::ThreadList;
use crate::api::thread::{AttachmentSummary, MessageDetail, ThreadDetail};
use crate::ui::{
    fmt_date, DateFormat, Icon, IconName, SavedQuery, SavedSidebarSection, SidebarRow,
    SidebarSection, TagBadge,
};

/// Full search page with results list and optional thread reader.
#[component]
pub fn SearchPage(
    query: String,
    results: ThreadList,
    selected_thread: Option<ThreadDetail>,
    total_messages: usize,
    total_threads: usize,
    tags: Vec<SidebarRow>,
    senders: Vec<SidebarRow>,
) -> Element {
    let threads = &results.threads;

    // Determine active tag index based on query (e.g., "tag:inbox")
    let active_tag = query.strip_prefix("tag:").map(|t| t.to_string());
    let tag_list: Vec<SidebarRow> = tags.into_iter().take(10).collect();
    let active_tag_idx = active_tag
        .as_ref()
        .and_then(|t| tag_list.iter().position(|row| &row.name == t));

    // Show username only (part before @) for senders, matching the mock
    let sender_list: Vec<SidebarRow> = senders
        .into_iter()
        .take(6)
        .map(|row| SidebarRow {
            name: row.name.split('@').next().unwrap_or(&row.name).to_string(),
            count: row.count,
        })
        .collect();

    rsx! {
        div { class: "i-main layout-3-col",
            aside { class: "i-sidebar scroll",
                SidebarSection {
                    title: String::from("TAGS"),
                    rows: tag_list,
                    active_idx: active_tag_idx,
                    linkbase: String::from("/search?q=tag:"),
                }
                SidebarSection {
                    title: String::from("SENDERS"),
                    rows: sender_list,
                    active_idx: None,
                    linkbase: String::from("/search?q=from:"),
                }
                SavedSidebarSection {
                    rows: vec![
                        SavedQuery {
                            display_name: String::from("receipts this year"),
                            query: String::from("tag:receipts date:2026-01..2026-12"),
                            count: 0,
                        },
                        SavedQuery {
                            display_name: String::from("with attachments"),
                            query: String::from("has:attachment"),
                            count: 0,
                        },
                        SavedQuery {
                            display_name: String::from("recent conversations"),
                            query: String::from("date:2026-03..2026-04"),
                            count: 0,
                        },
                    ],
                }
            }

            section { class: "i-results scroll",
                div { class: "i-results-head mono",
                    span { class: "col-marker" }
                    span { class: "col-date", "DATE" }
                    span { class: "col-from", "FROM" }
                    span { class: "col-subj", "SUBJECT · PREVIEW" }
                    span { class: "col-tags", "TAGS" }
                    span { class: "col-count", "MSGS" }
                }
                for (i , t) in threads.iter().enumerate() {
                    ThreadRow { thread: t.clone(), selected: i == 0 }
                }
            }

            section { class: "i-reader scroll",
                if let Some(detail) = selected_thread {
                    ReaderPane { detail }
                } else {
                    PlaceholderThread {}
                }
            }
        }
    }
}

#[component]
fn ThreadRow(thread: crate::api::search::ThreadSummary, selected: bool) -> Element {
    let date_str = fmt_date(thread.newest_date, DateFormat::Relative);
    let row_class = if selected { "i-row selected" } else { "i-row" };

    rsx! {
        div {
            class: "{row_class}",
            "data-thread-id": "{thread.thread_id}",
            span { class: "col-marker" }
            span { class: "col-date mono tnum", "{date_str}" }
            span { class: "col-from", "{thread.authors}" }
            span { class: "col-subj",
                span { class: "i-row-subj-text", "{thread.subject}" }
                if let Some(ref preview_text) = thread.preview {
                    span { class: "i-row-preview", " — {preview_text}" }
                }
            }
            span { class: "col-tags",
                for tag in thread.tags.iter().take(3) {
                    TagBadge { tag: tag.clone() }
                }
            }
            span { class: "col-count mono tnum",
                if thread.has_attachments {
                    Icon { name: IconName::Paperclip, size: Some(11) }
                }
                span { "{thread.matched_messages}/{thread.total_messages}" }
            }
        }
    }
}

#[component]
pub fn ReaderPane(detail: ThreadDetail) -> Element {
    let msg_count = detail.messages.len();

    let subject = detail
        .messages
        .first()
        .map_or_else(String::new, |m| m.subject.clone());

    // Compute thread metadata from messages
    let participant_count = {
        let mut unique = std::collections::HashSet::new();
        for m in &detail.messages {
            unique.insert(&m.headers.from);
        }
        unique.len()
    };
    let oldest_date = detail.messages.iter().map(|m| m.date).min();
    let newest_date = detail.messages.iter().map(|m| m.date).max();

    rsx! {
        div { class: "i-reader-head",
            div { class: "i-reader-eyebrow mono",
                span { "thread" }
                span { class: "i-reader-id", "{detail.thread_id}" }
            }
            h1 { class: "i-reader-subj", "{subject}" }
            div { class: "i-reader-meta mono",
                span {
                    b { class: "tnum", "{msg_count}" }
                    if msg_count == 1 { " message" } else { " messages" }
                }
                span { class: "i-sep-light", "·" }
                span {
                    b { class: "tnum", "{participant_count}" }
                    if participant_count == 1 { " participant" } else { " participants" }
                }
                if let (Some(oldest), Some(newest)) = (oldest_date, newest_date) {
                    span { class: "i-sep-light", "·" }
                    span {
                        "{fmt_date(oldest, DateFormat::Short)} → {fmt_date(newest, DateFormat::Short)}"
                    }
                }
                span { class: "i-reader-spacer" }
                for tag in detail.tags.iter() {
                    TagBadge { tag: tag.clone() }
                }
            }
            div { class: "i-reader-actions",
                a {
                    class: "i-btn",
                    href: "/api/message/{detail.thread_id}",
                    Icon { name: IconName::Download, size: Some(12) }
                    " raw .eml"
                }
                button { class: "i-btn",
                    Icon { name: IconName::Search, size: Some(12) }
                    " find in thread"
                }
                button { class: "i-btn", "data-action": "expand-all",
                    Icon { name: IconName::ChevronDown, size: Some(12) }
                    " expand all"
                }
                button { class: "i-btn i-btn-icon", title: "Copy permalink",
                    Icon { name: IconName::Link, size: Some(12) }
                }
            }
        }
        for (i , m) in detail.messages.iter().enumerate() {
            MessageBlock { message: m.clone(), idx: i + 1, total: msg_count, collapsed: false }
        }
    }
}

#[component]
fn MessageBlock(message: MessageDetail, idx: usize, total: usize, collapsed: bool) -> Element {
    let date_str = fmt_date(message.date, DateFormat::Iso);
    let has_atts = !message.attachments.is_empty();
    let idx_pad = format!("{idx:02}");
    let total_pad = format!("{total:02}");
    let msg_class = if collapsed {
        "i-msg collapsed"
    } else {
        "i-msg"
    };

    // Extract display name and email from "Name <email>" format
    let (from_name, from_email) = parse_from_header(&message.headers.from);

    rsx! {
        article { class: "{msg_class}", "data-message-id": "{message.message_id}",
            header { class: "i-msg-head",
                div { class: "i-msg-head-left",
                    span { class: "i-msg-num mono tnum",
                        "{idx_pad}"
                        span { class: "i-msg-num-sep", "/" }
                        "{total_pad}"
                    }
                    div { class: "i-msg-author",
                        div { class: "i-msg-author-name", "{from_name}" }
                        if !from_email.is_empty() {
                            div { class: "i-msg-author-email mono", "<{from_email}>" }
                        }
                    }
                }
                div { class: "i-msg-head-right mono",
                    span { class: "i-msg-to", "→ {message.headers.to}" }
                    span { class: "i-msg-date", "{date_str}" }
                    span { class: "i-msg-collapse",
                        Icon { name: IconName::ChevronDown, size: Some(12) }
                    }
                }
            }
            div { class: "i-msg-body",
                div { class: "prose", dangerous_inner_html: "{message.content}" }
                if has_atts {
                    div { class: "i-msg-atts",
                        div { class: "i-msg-atts-label mono",
                            Icon { name: IconName::Paperclip, size: Some(11) }
                            " {message.attachments.len()} attachment"
                            if message.attachments.len() != 1 { "s" }
                        }
                        for att in message.attachments.iter() {
                            AttachmentRow { attachment: att.clone(), message_id: message.message_id.clone() }
                        }
                    }
                }
            }
        }
    }
}

/// Parse a "Display Name <email@addr>" header into (name, email) parts.
fn parse_from_header(from: &str) -> (String, String) {
    if let Some(start) = from.find('<') {
        let name = from[..start].trim().to_string();
        let email = from
            .get(start + 1..)
            .and_then(|s| s.find('>').map(|end| s[..end].to_string()))
            .unwrap_or_default();
        (name, email)
    } else {
        (from.to_string(), String::new())
    }
}

#[component]
fn PlaceholderThread() -> Element {
    rsx! {
        div { class: "i-placeholder mono",
            div { class: "i-placeholder-line", "// select a thread to read" }
            div { class: "i-placeholder-text",
                "Click any thread in the results list to view its messages."
            }
        }
    }
}

#[component]
fn AttachmentRow(attachment: AttachmentSummary, message_id: String) -> Element {
    let name = attachment
        .filename
        .clone()
        .unwrap_or_else(|| "unnamed".into());
    let part = attachment.part;
    let ct = &attachment.content_type;
    let is_image = ct.starts_with("image/");
    let is_video = ct.starts_with("video/");
    let is_pdf = ct == "application/pdf" || name.ends_with(".pdf");
    let is_text =
        ct == "text/plain" || ct == "text/log" || name.ends_with(".log") || name.ends_with(".txt");
    let download_url = format!(
        "/api/attachment?msg={}&part={}",
        urlencoding::encode(&message_id),
        part
    );

    let icon_name = if is_image {
        IconName::Image
    } else {
        IconName::File
    };

    rsx! {
        div { class: "i-msg-att",
            div { class: "i-att-chrome",
                div { class: "i-att-chrome-bar",
                    a {
                        class: "i-att-chrome-info",
                        href: "{download_url}",
                        target: "_blank",
                        title: "Open {name}",
                        Icon { name: icon_name.clone(), size: Some(12) }
                        " {name} · {ct}"
                    }
                    span { class: "i-att-chrome-actions",
                        a {
                            href: "{download_url}",
                            target: "_blank",
                            title: "Download",
                            class: "i-att-chrome-dl",
                            Icon { name: IconName::Download, size: Some(14) }
                        }
                    }
                }
                if is_pdf {
                    iframe {
                        src: "{download_url}",
                        class: "i-att-pdf-frame",
                        title: "PDF preview: {name}",
                    }
                } else if is_image {
                    div { class: "i-att-img-wrap",
                        img {
                            src: "{download_url}",
                            alt: "{name}",
                            class: "i-att-img",
                        }
                    }
                } else if is_video {
                    div { class: "i-att-video-wrap",
                        video {
                            src: "{download_url}",
                            class: "i-att-video",
                            controls: true,
                            preload: "metadata",
                        }
                    }
                } else if is_text {
                    iframe {
                        src: "{download_url}",
                        class: "i-att-log-frame",
                        title: "Text preview: {name}",
                    }
                }
            }
        }
    }
}
