//! Home / help page with notmuch search syntax reference.

use dioxus::prelude::*;

#[component]
pub fn HomePage() -> Element {
    rsx! {
        section { class: "i-reader scroll", style: "flex: 1; min-height: 0;",
            div { class: "i-reader-head",
                div { class: "i-reader-eyebrow mono", "RUMMAGE" }
                h1 { class: "i-reader-subj", "Email Archive Search" }
                p { class: "prose",
                    "Rummage is a fast, local-only email archive browser powered by ",
                    a { href: "https://notmuchmail.org/", "notmuch" },
                    ". All data stays on your machine."
                }
            }

            div { class: "i-msg-body",
                h2 { class: "i-reader-subj", style: "font-size: 16px; margin-top: 24px;",
                    "Search Syntax"
                }
                div { class: "prose",
                    p { "Rummage uses notmuch query syntax. Here are the most useful operators:" }

                    table { style: "width: 100%; font-size: 12px; border-collapse: collapse; margin-top: 12px;",
                        thead {
                            tr { style: "border-bottom: 1px solid var(--line); text-align: left;",
                                th { style: "padding: 6px 8px;", "Operator" }
                                th { style: "padding: 6px 8px;", "Description" }
                                th { style: "padding: 6px 8px;", "Example" }
                            }
                        }
                        tbody {
                            for row in HELP_ROWS {
                                tr { style: "border-bottom: 1px solid var(--line-2);",
                                    td { style: "padding: 6px 8px; font-family: var(--font-mono);",
                                        code { "{row.0}" }
                                    }
                                    td { style: "padding: 6px 8px;", "{row.1}" }
                                    td { style: "padding: 6px 8px; font-family: var(--font-mono); color: var(--fg-3);",
                                        "{row.2}"
                                    }
                                }
                            }
                        }
                    }

                    p { style: "margin-top: 16px;",
                        "Combine operators with ",
                        code { "AND" },
                        ", ",
                        code { "OR" },
                        ", and ",
                        code { "NOT" },
                        ". Use quotes for exact phrases."
                    }
                }

                div { style: "margin-top: 32px; padding-top: 16px; border-top: 1px solid var(--line);",
                    h2 { class: "i-reader-subj", style: "font-size: 16px;",
                        "Keyboard Shortcuts"
                    }
                    div { class: "prose",
                        p {
                            span { class: "kbd", "j" }
                            " / "
                            span { class: "kbd", "k" }
                            " — next / previous thread"
                        }
                        p {
                            span { class: "kbd", "o" }
                            " — open selected thread"
                        }
                        p {
                            span { class: "kbd", "⌘" }
                            span { class: "kbd", "K" }
                            " — open command palette"
                        }
                        p {
                            span { class: "kbd", "/" }
                            " — focus search"
                        }
                        p {
                            span { class: "kbd", "?" }
                            " — show this help page"
                        }
                    }
                }
            }
        }
    }
}

const HELP_ROWS: &[(&str, &str, &str)] = &[
    ("from:", "Sender address or name", "from:bob@example.com"),
    ("to:", "Recipient address", "to:me@example.com"),
    ("subject:", "Subject line contains", "subject:invoice"),
    ("tag:", "Tagged with label", "tag:work AND tag:important"),
    (
        "has:attachment",
        "Has at least one attachment",
        "has:attachment",
    ),
    (
        "date:..",
        "Date range (YYYY-MM-DD)",
        "date:2026-01-01..2026-03-31",
    ),
    (
        "thread:{id}",
        "Specific thread ID",
        "thread:0000000000000001",
    ),
    ("id:{msgid}", "Specific message ID", "id:abc123@example.com"),
];
