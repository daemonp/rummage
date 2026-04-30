//! **Rummage** — A local-only email archive search server.
//!
//! Rummage indexes email archives via [notmuch](https://notmuchmail.org/) and
//! serves both server-rendered HTML (via Dioxus SSR) and a JSON REST API
//! (via Axum).  All data stays on the local machine.
//!
//! # Architecture
//!
//! - **[`db`]** — Single `spawn_blocking` worker for all `libnotmuch` access.
//! - **[`api`]** — JSON API handlers (search, thread, attachment, message, tags, stats).
//! - **[`ui`]** — Dioxus SSR components for the HTML frontend.
//! - **[`mail`]** — Email parsing, body sanitization, and attachment extraction.
//! - **[`server`]** — Axum router, middleware, and HTML page handlers.
//! - **[`assets`]** — Compile-time embedded CSS/JS/SVG.

pub mod api;
pub mod assets;
pub mod config;
pub mod db;
pub mod error;
pub mod mail;
pub mod mcp;
pub mod server;
pub mod ui;
