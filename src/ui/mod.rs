//! Dioxus UI components for server-side rendering.
//!
//! All components render to HTML strings via `dioxus_ssr::render_element`.
//! The same component definitions can be reused for native/desktop targets later.

use dioxus::prelude::*;

// Re-export page modules
pub mod home;
pub mod search;
pub mod thread;

use crate::assets::{INSTRUMENT_CSS, TOKENS_CSS};

/// App-specific CSS overrides.
///
/// The designer's mock uses `<div>` for clickable elements (React with onClick),
/// but our SSR renders real `<a>` tags for navigation. We reset anchor
/// styles so they inherit the design system colors instead of browser defaults.
static APP_CSS: &str = r#"
/* Anchor tag reset — mock uses divs, SSR uses <a> for sidebar links and buttons */
a { color: inherit; text-decoration: none; }
a:visited { color: inherit; }
a:hover { color: inherit; }
a:active { color: inherit; }

/* Sidebar anchor rows inherit the mock's div-based hover/active behavior */
a.i-side-row:hover .i-side-name { color: var(--fg); }
a.i-side-row.active .i-side-name { color: var(--accent); }

/* Search form: icon should not shrink, and form SVG should match trigger color */
.i-search-form > svg { flex-shrink: 0; color: var(--fg-3); }
"#;

/// Render a complete HTML document wrapping the given Dioxus element.
///
/// Returns a `String` containing `<!DOCTYPE html>…` with embedded CSS.
/// User-controlled values (`title`, `theme`) are HTML-escaped to prevent XSS.
pub fn render_page(title: &str, element: Element, theme: &str) -> String {
    let body_html = dioxus_ssr::render_element(element);
    let theme_class = if theme == "light" {
        "theme-instrument light"
    } else {
        "theme-instrument dark"
    };
    // HTML-escape user-controlled values to prevent XSS injection.
    let safe_title = html_escape::encode_text(title);
    let safe_theme = html_escape::encode_double_quoted_attribute(theme);
    format!(
        r#"<!DOCTYPE html>
<html lang="en" data-theme="{safe_theme}">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<title>{safe_title}</title>
<style>
{tokens}
</style>
<style>
{instrument}
</style>
<style>
{app}
</style>
</head>
<body class="{theme_class}">
{body}
<script src="/app.js" defer></script>
</body>
</html>"#,
        safe_title = safe_title,
        safe_theme = safe_theme,
        theme_class = theme_class,
        tokens = TOKENS_CSS,
        instrument = INSTRUMENT_CSS,
        app = APP_CSS,
        body = body_html,
    )
}

// ── Icons (1.4px stroke, 16px viewBox) ─────────────────────────────

/// Compile-time-checked icon names used by the [`Icon`] component.
///
/// Using an enum prevents typos (no silent fallback to a dot) and avoids
/// a `String` allocation at every call site.
#[derive(Debug, Clone, PartialEq)]
pub enum IconName {
    Search,
    Inbox,
    Star,
    Paperclip,
    ChevronRight,
    ChevronDown,
    Download,
    File,
    Image,
    Mail,
    Tag,
    User,
    Calendar,
    ArrowLeft,
    ArrowRight,
    Sliders,
    X,
    Archive,
    Trash,
    Plus,
    Eye,
    Filter,
    Quote,
    Sort,
    Check,
    Dot,
    Reply,
    Forward,
    Link,
}

impl IconName {
    /// SVG path data for this icon (16×16 viewBox, 1.4px stroke).
    fn path_data(&self) -> &'static str {
        match self {
            Self::Search => "M14 14l-3.5-3.5M11.5 6.5a5 5 0 1 1-10 0 5 5 0 0 1 10 0z",
            Self::Inbox => "M2 9l1.5-5.5A1 1 0 0 1 4.5 3h7a1 1 0 0 1 1 0.7L14 9M2 9v3.5a.5.5 0 0 0 .5.5h11a.5.5 0 0 0 .5-.5V9M2 9h3.5l1 2h3l1-2H14",
            Self::Star => "M8 1.5l2 4.5 5 .5-3.7 3.4 1.1 5L8 12.3l-4.4 2.6 1.1-5L1 6.5l5-.5z",
            Self::Paperclip => "M11.5 7L6 12.5a2.5 2.5 0 1 1-3.5-3.5L8 3.5a1.5 1.5 0 1 1 2 2L4.5 11",
            Self::ChevronRight => "M6 4l4 4-4 4",
            Self::ChevronDown => "M4 6l4 4 4-4",
            Self::Download => "M8 2v9M4.5 7.5L8 11l3.5-3.5M3 13h10",
            Self::File => "M3 2h6l4 4v8H3z M9 2v4h4",
            Self::Image => "M2 3h12v10H2z M5.5 6.5a1 1 0 1 1-2 0 1 1 0 0 1 2 0 M2 11l3-3 3 3 2-2 4 4",
            Self::Mail => "M2 4h12v8H2z M2 4l6 4 6-4",
            Self::Tag => "M2 2h6l6 6-6 6-6-6z M5 5h.01",
            Self::User => "M3 13c0-2 2-3.5 5-3.5s5 1.5 5 3.5 M8 8a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z",
            Self::Calendar => "M3 4h10v9H3z M3 7h10 M5 2v3 M11 2v3",
            Self::ArrowLeft => "M10 12L6 8l4-4",
            Self::ArrowRight => "M6 4l4 4-4 4",
            Self::Sliders => "M2 4h6 M10 4h4 M2 8h2 M6 8h8 M2 12h10 M12 12h2 M9 4a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z M5 8a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z M11 12a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z",
            Self::X => "M3 3l10 10 M13 3L3 13",
            Self::Archive => "M2 3h12v3H2z M3 6v8h10V6 M6 9h4",
            Self::Trash => "M3 4h10 M5 4V2h6v2 M5 4l1 10h4l1-10",
            Self::Plus => "M8 3v10 M3 8h10",
            Self::Eye => "M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5z M8 10.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z",
            Self::Filter => "M1 3h14l-5.5 6.5V14L6.5 12V9.5z",
            Self::Quote => "M3 6h3v3l-2 3H3V9zm6 0h3v3l-2 3H9V9z",
            Self::Sort => "M4 3v10 M2 11l2 2 2-2 M12 13V3 M14 5l-2-2-2 2",
            Self::Check => "M3 8l3 3 7-7",
            Self::Dot => "M8 8m-2 0a2 2 0 1 0 4 0 2 2 0 1 0 -4 0",
            Self::Reply => "M6 4L2 8l4 4 M2 8h7a4 4 0 0 1 4 4v1",
            Self::Forward => "M10 4l4 4-4 4 M14 8H7a4 4 0 0 0-4 4v1",
            Self::Link => "M7 9a3 3 0 0 0 4.2 0l2.5-2.5a3 3 0 1 0-4.2-4.2L8 4 M9 7a3 3 0 0 0-4.2 0L2.3 9.5a3 3 0 1 0 4.2 4.2L8 12",
        }
    }
}

/// Inline SVG icon component.
#[component]
pub fn Icon(name: IconName, size: Option<u32>) -> Element {
    let size = size.unwrap_or(16);
    let d = name.path_data();
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 16 16",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "1.4",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            path { d: "{d}" }
        }
    }
}

// ── Formatting helpers ───────────────────────────────────────────────

/// Date display format for [`fmt_date`].
pub enum DateFormat {
    /// Relative: "5m", "3h", "2d", or "Feb 10".
    Relative,
    /// ISO-like: "2026-04-30 14:05".
    Iso,
    /// Short: "Apr 30 '26".
    Short,
}

/// Format a Unix timestamp according to the given [`DateFormat`].
pub fn fmt_date(timestamp: i64, mode: DateFormat) -> String {
    use chrono::{DateTime, Datelike, Local};

    let dt = match DateTime::from_timestamp(timestamp, 0) {
        Some(d) => d.with_timezone(&Local),
        None => return "unknown".into(),
    };

    match mode {
        DateFormat::Relative => {
            let now = Local::now();
            let secs = now.signed_duration_since(dt).num_seconds();
            if secs < 0 {
                return "just now".into();
            }
            if secs < 3600 {
                return format!("{}m", secs / 60);
            }
            if secs < 86400 {
                return format!("{}h", secs / 3600);
            }
            if secs < 86400 * 7 {
                return format!("{}d", secs / 86400);
            }
            if dt.year() == now.year() {
                dt.format("%b %d").to_string()
            } else {
                dt.format("%b %d '%y").to_string()
            }
        }
        DateFormat::Iso => dt.format("%Y-%m-%d %H:%M").to_string(),
        DateFormat::Short => dt.format("%b %d '%y").to_string(),
    }
}

/// Format a number with comma-separated thousands (e.g., 1,234,567).
pub(crate) fn fmt_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

// ── Shared layout components ───────────────────────────────────────

#[component]
pub fn AppShell(children: Element, stats: Option<String>) -> Element {
    rsx! {
        div { class: "app instrument-app",
            {children}
            StatusBar { stats: stats.unwrap_or_default() }
        }
    }
}

#[component]
pub fn StatusBar(stats: String) -> Element {
    rsx! {
        footer { class: "i-status mono",
            span { class: "i-status-ok",
                "● libnotmuch · read-only"
            }
            span { class: "i-status-spacer" }
            span {
                span { class: "kbd", "j" }
                span { class: "kbd", "k" }
                " next/prev"
            }
            span {
                span { class: "kbd", "o" }
                " open"
            }
            span {
                span { class: "kbd", "⌘" }
                span { class: "kbd", "K" }
                " command"
            }
            span {
                span { class: "kbd", "/" }
                " find"
            }
            span {
                span { class: "kbd", "u" }
                " raw .eml"
            }
            span {
                span { class: "kbd", "?" }
                " help"
            }
            if !stats.is_empty() {
                span { class: "i-status-spacer" }
                span { "{stats}" }
            }
            span { class: "i-status-spacer" }
            button {
                class: "i-theme-toggle",
                "aria-label": "Toggle theme",
                "data-theme-action": "toggle",
                Icon { name: IconName::Eye, size: Some(12) }
            }
        }
    }
}

#[component]
pub fn Header(
    query: String,
    total_messages: Option<usize>,
    total_threads: Option<usize>,
) -> Element {
    let total_messages = total_messages.unwrap_or(0);
    let total_threads = total_threads.unwrap_or(0);
    let msgs_str = fmt_number(total_messages);
    let threads_str = fmt_number(total_threads);
    rsx! {
        header { class: "i-header",
            div { class: "i-header-left",
                div { class: "i-logo",
                    span { class: "i-logo-mark", "▮" }
                    span { class: "i-logo-text", "RUMMAGE" }
                }
                if total_messages > 0 {
                    div { class: "i-stats mono",
                        span {
                            b { class: "tnum", "{msgs_str}" }
                            span { class: "i-stats-unit", "msgs" }
                        }
                        span { class: "i-sep", "/" }
                        span {
                            b { class: "tnum", "{threads_str}" }
                            span { class: "i-stats-unit", "threads" }
                        }
                    }
                }
            }
            form {
                class: "i-search-form",
                method: "GET",
                action: "/search",
                Icon { name: IconName::Search, size: Some(14) }
                input {
                    class: "i-search-input mono",
                    r#type: "text",
                    name: "q",
                    value: "{query}",
                    placeholder: "Search the archive…",
                    "aria-label": "Search query"
                }
                span { class: "i-search-trigger-kbd",
                    span { class: "kbd", "⌘" }
                    span { class: "kbd", "K" }
                }
            }
        }
    }
}

#[component]
pub fn Subheader(
    query: String,
    thread_count: usize,
    message_count: Option<usize>,
    search_time_ms: Option<u64>,
) -> Element {
    let msg_count = message_count.unwrap_or(0);
    let time_ms = search_time_ms.unwrap_or(0);
    rsx! {
        div { class: "i-subheader mono",
            span { class: "i-subheader-label", "QUERY" }
            code { class: "i-subheader-q", "{query}" }
            span { class: "i-subheader-meta",
                b { class: "tnum", "{thread_count}" }
                " threads"
                if msg_count > 0 {
                    span { class: "i-sep-light", "·" }
                    b { class: "tnum", "{msg_count}" }
                    " messages"
                }
                if time_ms > 0 {
                    span { class: "i-sep-light", "·" }
                    b { class: "tnum", "{time_ms}" }
                    span { class: "i-stats-unit", "ms" }
                }
            }
            span { class: "i-subheader-spacer" }
            FilterChip { label: String::from("from"), value: String::from("any") }
            FilterChip { label: String::from("date"), value: String::from("all") }
            FilterChip { label: String::from("attach"), value: String::from("any") }
            FilterChip { label: String::from("sort"), value: String::from("date ↓") }
        }
    }
}

#[component]
pub fn FilterChip(label: String, value: String) -> Element {
    rsx! {
        span { class: "i-chip",
            span { class: "i-chip-label", "{label}" }
            span { class: "i-chip-value", "{value}" }
            Icon { name: IconName::ChevronDown, size: Some(10) }
        }
    }
}

// ── Sidebar ────────────────────────────────────────────────────────

/// A row in a sidebar section (tag, sender, etc.).
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarRow {
    pub name: String,
    pub count: usize,
}

/// A saved query displayed in the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedQuery {
    pub display_name: String,
    pub query: String,
    pub count: usize,
}

#[component]
pub fn SidebarSection(
    title: String,
    rows: Vec<SidebarRow>,
    active_idx: Option<usize>,
    linkbase: String,
) -> Element {
    let active = active_idx.unwrap_or(usize::MAX);
    rsx! {
        div { class: "i-side-section",
            div { class: "i-side-title mono", "{title}" }
            for (i , row) in rows.iter().enumerate() {
                {
                    let count_str = fmt_number(row.count);
                    rsx! {
                        a {
                            class: if i == active { "i-side-row active mono" } else { "i-side-row mono" },
                            href: "{linkbase}{row.name}",
                            span { class: "i-side-name", "{row.name}" }
                            if row.count > 0 {
                                span { class: "i-side-count tnum", "{count_str}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn TagBadge(tag: String) -> Element {
    rsx! {
        span { class: "tag", "{tag}" }
    }
}

/// Sidebar section for saved queries with separate display names and query values.
#[component]
pub fn SavedSidebarSection(rows: Vec<SavedQuery>) -> Element {
    rsx! {
        div { class: "i-side-section",
            div { class: "i-side-title mono", "SAVED" }
            for row in rows.iter() {
                {
                    let count_str = fmt_number(row.count);
                    rsx! {
                        a {
                            class: "i-side-row mono",
                            href: "/search?q={row.query}",
                            span { class: "i-side-name", "{row.display_name}" }
                            if row.count > 0 {
                                span { class: "i-side-count tnum", "{count_str}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dioxus_ssr() {
        let html = dioxus_ssr::render_element(rsx! { Hello { name: String::from("World") } });
        assert!(html.contains("Hello World"), "got: {}", html);
    }

    #[component]
    fn Hello(name: String) -> Element {
        rsx! { div { "Hello {name}" } }
    }

    #[test]
    fn test_render_page_wraps_doctype() {
        let html = render_page("Test", rsx! { div { "hi" } }, "dark");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Test</title>"));
        assert!(html.contains("hi"));
    }

    #[test]
    fn test_render_page_light_theme() {
        let html = render_page("Test", rsx! { div { "hi" } }, "light");
        assert!(html.contains("theme-instrument light"));
    }

    #[test]
    fn test_render_page_escapes_xss_in_title() {
        let html = render_page("</title><script>alert(1)</script>", rsx! { div {} }, "dark");
        assert!(!html.contains("<script>alert(1)</script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_fmt_number() {
        assert_eq!(fmt_number(0), "0");
        assert_eq!(fmt_number(999), "999");
        assert_eq!(fmt_number(1000), "1,000");
        assert_eq!(fmt_number(1_000_000), "1,000,000");
    }
}
