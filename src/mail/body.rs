use ammonia::Builder;
use linkify::{LinkFinder, LinkKind};
use regex::Regex;
use std::fmt::Write;
use std::sync::LazyLock;

/// Sanitize raw HTML email body, allowing only safe tags and attributes.
#[must_use]
pub fn sanitize_html(raw: &str) -> String {
    let mut builder = Builder::default();
    builder
        .tags(
            [
                "a",
                "abbr",
                "b",
                "blockquote",
                "br",
                "code",
                "div",
                "em",
                "i",
                "li",
                "ol",
                "p",
                "pre",
                "span",
                "strong",
                "ul",
                "table",
                "thead",
                "tbody",
                "tr",
                "td",
                "th",
                "img",
                "h1",
                "h2",
                "h3",
                "h4",
                "h5",
                "h6",
                "hr",
                "dl",
                "dt",
                "dd",
                "font",
                "center",
                "address",
            ]
            .iter()
            .copied()
            .collect(),
        )
        .tag_attributes(
            [
                ("a", ["href", "title", "name"].iter().copied().collect()),
                (
                    "img",
                    ["src", "alt", "title", "width", "height"]
                        .iter()
                        .copied()
                        .collect(),
                ),
                ("abbr", std::iter::once(&"title").copied().collect()),
            ]
            .iter()
            .cloned()
            .collect(),
        )
        .url_relative(ammonia::UrlRelative::PassThrough)
        .clean(raw)
        .to_string()
}

/// Convert plain text email body to safe HTML.
/// - Escape HTML entities
/// - Nest blockquotes for `>` prefixes
/// - Auto-link URLs and emails
/// - Convert newlines to `<br>` or wrap paragraphs
#[must_use]
pub fn text_to_html(raw: &str) -> String {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url, LinkKind::Email]);

    let mut result = String::new();
    let mut in_blockquote = 0usize;

    for line in raw.lines() {
        let (depth, content) = count_quote_depth(line);

        // Close/open blockquotes to match depth
        while in_blockquote > depth {
            result.push_str("</blockquote>");
            in_blockquote -= 1;
        }
        while in_blockquote < depth {
            result.push_str("<blockquote>");
            in_blockquote += 1;
        }

        // Escape HTML entities, then auto-link
        let escaped = html_escape::encode_text(content);
        let linked = auto_link(&escaped, &finder);
        result.push_str(&linked);
        result.push_str("<br>\n");
    }

    while in_blockquote > 0 {
        result.push_str("</blockquote>");
        in_blockquote -= 1;
    }

    result
}

fn count_quote_depth(line: &str) -> (usize, &str) {
    let mut depth = 0usize;
    let mut rest = line;
    while let Some(stripped) = rest.strip_prefix('>') {
        depth += 1;
        rest = stripped;
        // optional space after >
        if let Some(after_space) = rest.strip_prefix(' ') {
            rest = after_space;
        }
    }
    (depth, rest)
}

fn auto_link(text: &str, finder: &LinkFinder) -> String {
    let mut result = String::new();
    let mut last_end = 0;

    for link in finder.links(text) {
        result.push_str(&text[last_end..link.start()]);
        let url = link.as_str();
        let href = if link.kind() == &LinkKind::Email {
            format!("mailto:{url}")
        } else if !url.starts_with("http://") && !url.starts_with("https://") {
            format!("https://{url}")
        } else {
            url.to_string()
        };
        let _ = write!(
            result,
            "<a href=\"{}\" rel=\"noopener noreferrer\" target=\"_blank\">{}</a>",
            html_escape::encode_quoted_attribute(&href),
            url
        );
        last_end = link.end();
    }
    result.push_str(&text[last_end..]);
    result
}

// ── HTML → text / markdown helpers ──────────────────────────────────

static HTML_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]+>").expect("static HTML tag regex is valid"));

/// Strip HTML tags and decode common entities to produce plain text.
/// Preserves paragraph breaks by replacing `<br>` and `</p>` with `\n`.
#[must_use]
pub fn html_to_text(html: &str) -> String {
    let mut text = html.to_string();

    // Normalise block-level whitespace before stripping tags.
    text = text.replace("<br>", "\n");
    text = text.replace("<br/>", "\n");
    text = text.replace("<br />", "\n");
    text = text.replace("</p>", "\n\n");
    text = text.replace("<p>", "");

    // Strip remaining tags.
    text = HTML_TAG_RE.replace_all(&text, "").into_owned();

    decode_basic_entities(&text)
}

/// Convert (sanitised) HTML to basic Markdown.
#[must_use]
pub fn html_to_markdown(html: &str) -> String {
    let mut md = String::new();
    let mut in_link_href: Option<String> = None;
    let mut list_depth = 0usize;
    let mut ordered_stack = Vec::new(); // true = <ol>, false = <ul>
    let mut list_counter = Vec::new();

    // Simple regex-based tokeniser for common tags.
    let tag_re = Regex::new(r"<(/?)([a-zA-Z0-9]+)([^>]*)>").expect("static tag regex is valid");
    let mut last_end = 0;

    for caps in tag_re.captures_iter(html) {
        let full_match = caps.get(0).unwrap();
        md.push_str(&html[last_end..full_match.start()]);

        let closing = !caps.get(1).unwrap().as_str().is_empty();
        let tag = caps.get(2).unwrap().as_str().to_ascii_lowercase();
        let attrs = caps.get(3).unwrap().as_str();

        match tag.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = tag.chars().last().unwrap().to_digit(10).unwrap_or(1) as usize;
                if !closing {
                    md.push_str(&"#".repeat(level));
                    md.push(' ');
                } else {
                    md.push('\n');
                }
            }
            "b" | "strong" => {
                md.push_str("**");
            }
            "i" | "em" => {
                md.push('*');
            }
            "a" => {
                if closing {
                    if let Some(href) = in_link_href.take() {
                        md.push_str(&format!("]({href})"));
                    }
                } else {
                    if let Some(href) = extract_attr(attrs, "href") {
                        in_link_href = Some(href);
                        md.push('[');
                    }
                }
            }
            "ul" => {
                if closing {
                    if !ordered_stack.is_empty() {
                        ordered_stack.pop();
                    }
                    if !list_counter.is_empty() {
                        list_counter.pop();
                    }
                    list_depth = list_depth.saturating_sub(1);
                    md.push('\n');
                } else {
                    ordered_stack.push(false);
                    list_counter.push(0usize);
                    list_depth += 1;
                }
            }
            "ol" => {
                if closing {
                    if !ordered_stack.is_empty() {
                        ordered_stack.pop();
                    }
                    if !list_counter.is_empty() {
                        list_counter.pop();
                    }
                    list_depth = list_depth.saturating_sub(1);
                    md.push('\n');
                } else {
                    ordered_stack.push(true);
                    list_counter.push(1usize);
                    list_depth += 1;
                }
            }
            "li" => {
                if !closing {
                    let indent = "  ".repeat(list_depth.saturating_sub(1));
                    if ordered_stack.last() == Some(&true) {
                        let num = if let Some(n) = list_counter.last_mut() {
                            let current = *n;
                            *n += 1;
                            current
                        } else {
                            0usize
                        };
                        md.push_str(&format!("{indent}{num}. "));
                    } else {
                        md.push_str(&format!("{indent}- "));
                    }
                } else {
                    md.push('\n');
                }
            }
            "br" => {
                md.push('\n');
            }
            "p" if closing => {
                md.push_str("\n\n");
            }
            _ => {} // strip other tags
        }

        last_end = full_match.end();
    }

    md.push_str(&html[last_end..]);
    decode_basic_entities(&md)
}

/// Truncate text at a word boundary, appending a custom suffix.
#[must_use]
pub fn truncate_text_with_suffix(text: &str, max_bytes: usize, suffix: &str) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    // Walk back to a word boundary.
    while end > 0 {
        if text[..end].ends_with(' ') || text[..end].ends_with('\n') || text[..end].ends_with('\t')
        {
            // trim trailing whitespace
            while end > 0
                && (text.as_bytes()[end - 1] == b' '
                    || text.as_bytes()[end - 1] == b'\n'
                    || text.as_bytes()[end - 1] == b'\t')
            {
                end -= 1;
            }
            break;
        }
        end -= 1;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
    }
    if end == 0 {
        end = max_bytes;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
    }
    format!("{}{}", &text[..end], suffix)
}

/// Truncate text at a word boundary, appending `[truncated...]`.
#[must_use]
pub fn truncate_text(text: &str, max_chars: usize) -> String {
    truncate_text_with_suffix(text, max_chars, " [truncated...]")
}

fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"{}\s*=\s*"([^"]*)""#, regex::escape(name)))
        .ok()
        .or_else(|| Regex::new(&format!(r#"{}\s*=\s*'([^']*)'"#, regex::escape(name))).ok())
        .or_else(|| Regex::new(&format!(r#"{}\s*=\s*([^\s>]+)"#, regex::escape(name))).ok());
    if let Some(re) = re {
        re.captures(attrs).and_then(|c| {
            c.get(1).map(|m| {
                let s = m.as_str();
                if s.starts_with('\'') && s.ends_with('\'')
                    || s.starts_with('"') && s.ends_with('"')
                {
                    s[1..s.len() - 1].to_string()
                } else {
                    s.to_string()
                }
            })
        })
    } else {
        None
    }
}

fn decode_basic_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&#x27;", "'")
        .replace("&#x22;", "\"")
        .replace("&#x3C;", "<")
        .replace("&#x3E;", ">")
        .replace("&#x26;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── text_to_html ───────────────────────────────────────────────────

    #[test]
    fn test_plain_text_to_html_quotes() {
        let input = "Hello world\n> quote\n>> nested\nback\n";
        let html = text_to_html(input);
        println!("HTML output:\n{html}");
        assert!(html.contains("Hello world"));
        assert!(html.contains("<blockquote>"));
        assert!(html.contains("</blockquote>"));
    }

    #[test]
    fn test_url_autolink() {
        let input = "Check out https://example.com";
        let html = text_to_html(input);
        assert!(html.contains(r#"<a href="https://example.com""#));
    }

    #[test]
    fn test_email_autolink() {
        let input = "Contact me@example.com";
        let html = text_to_html(input);
        assert!(html.contains(r#"<a href="mailto:me@example.com""#));
    }

    #[test]
    fn test_html_escaping() {
        let input = "<script>alert('xss')</script>";
        let html = text_to_html(input);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    // ── html_to_text ─────────────────────────────────────────────────────

    #[test]
    fn test_html_to_text_simple() {
        let html = "<p>Hello <b>world</b></p>";
        assert_eq!(html_to_text(html), "Hello world\n\n");
    }

    #[test]
    fn test_html_to_text_entities() {
        let html = "&lt;div&gt;Hello &amp; goodbye&lt;/div&gt;";
        assert_eq!(html_to_text(html), "<div>Hello & goodbye</div>");
    }

    #[test]
    fn test_html_to_text_paragraphs() {
        let html = "<p>First paragraph</p><p>Second paragraph</p>";
        assert_eq!(
            html_to_text(html),
            "First paragraph\n\nSecond paragraph\n\n"
        );
    }

    #[test]
    fn test_html_to_text_line_breaks() {
        let html = "Line one<br>Line two<br/>Line three<br />Line four";
        assert_eq!(
            html_to_text(html),
            "Line one\nLine two\nLine three\nLine four"
        );
    }

    #[test]
    fn test_html_to_text_nested_tags() {
        let html = "<div><p>Text in <span>nested</span> tags</p></div>";
        assert_eq!(html_to_text(html), "Text in nested tags\n\n");
    }

    #[test]
    fn test_html_to_text_links() {
        let html = r#"<a href="https://example.com">Click here</a>"#;
        assert_eq!(html_to_text(html), "Click here");
    }

    // ── html_to_markdown ───────────────────────────────────────────────

    #[test]
    fn test_html_to_markdown_headings() {
        let html = "<h1>Title</h1><h2>Subtitle</h2><h3>Section</h3>";
        assert_eq!(
            html_to_markdown(html),
            "# Title\n## Subtitle\n### Section\n"
        );
    }

    #[test]
    fn test_html_to_markdown_bold_strong() {
        let html = "<b>Bold</b> and <strong>Strong</strong>";
        assert_eq!(html_to_markdown(html), "**Bold** and **Strong**");
    }

    #[test]
    fn test_html_to_markdown_italic_em() {
        let html = "<i>Italic</i> and <em>Emphasis</em>";
        assert_eq!(html_to_markdown(html), "*Italic* and *Emphasis*");
    }

    #[test]
    fn test_html_to_markdown_links() {
        let html = r#"<a href="https://example.com">Link text</a>"#;
        assert_eq!(html_to_markdown(html), "[Link text](https://example.com)");
    }

    #[test]
    fn test_html_to_markdown_unordered_list() {
        let html = "<ul><li>Item 1</li><li>Item 2</li></ul>";
        assert_eq!(html_to_markdown(html), "- Item 1\n- Item 2\n\n");
    }

    #[test]
    fn test_html_to_markdown_ordered_list() {
        let html = "<ol><li>First</li><li>Second</li></ol>";
        assert_eq!(html_to_markdown(html), "1. First\n2. Second\n\n");
    }

    #[test]
    fn test_html_to_markdown_tables() {
        let html = "<table><tr><th>Header</th></tr><tr><td>Cell</td></tr></table>";
        let md = html_to_markdown(html);
        // Tables: strip tags but preserve text content (markdown table format
        // would require a more sophisticated parser).
        assert!(md.contains("Header"));
        assert!(md.contains("Cell"));
        assert!(!md.contains("<table>"));
        assert!(!md.contains("<tr>"));
    }

    #[test]
    fn test_html_to_markdown_code_blocks() {
        let html = "<pre><code>let x = 1;</code></pre>";
        let md = html_to_markdown(html);
        assert!(md.contains("let x = 1;"));
        assert!(!md.contains("<pre>"));
        assert!(!md.contains("<code>"));
    }

    // ── truncate_text ──────────────────────────────────────────────────

    #[test]
    fn test_truncate_text_word_boundary() {
        let text = "This is a long sentence that needs truncation";
        let truncated = truncate_text(text, 20);
        assert!(truncated.ends_with("[truncated...]"));
        assert!(truncated.starts_with("This is a long"));
        assert!(!truncated.contains("sentence"));
    }

    #[test]
    fn test_truncate_text_shorter_than_limit() {
        let text = "Short";
        assert_eq!(truncate_text(text, 100), "Short");
    }

    #[test]
    fn test_truncate_text_exact_length() {
        let text = "Exactly twenty chars";
        assert_eq!(truncate_text(text, 20), "Exactly twenty chars");
    }

    #[test]
    fn test_truncate_text_multibyte() {
        let text = "Hello 世界 this is a long text";
        let truncated = truncate_text(text, 10);
        assert!(truncated.ends_with("[truncated...]"));
        // Must not panic or split in the middle of a multibyte character.
        assert!(truncated.is_char_boundary(truncated.len() - "[truncated...]".len()));
    }
}
