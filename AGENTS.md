# AGENTS.md — Rummage Project Conventions

## Project Overview

Rummage is a Rust HTTP server for browsing and searching email archives indexed by notmuch. It serves server-rendered HTML (Dioxus SSR), a JSON REST API (Axum), and an MCP server (rmcp) — all from a single binary with zero runtime file dependencies.

## Build Prerequisites

- **Rust stable 1.75+**
- **`libnotmuch-dev`** system library (Debian: `apt-get install libnotmuch-dev`, Arch: `pacman -S notmuch`)
- **`assets/` directory** must exist with `tokens.css`, `styles-directions.css`, `app.js`, `favicon.svg`. These are embedded at compile time via `include_str!` in `src/assets.rs`. Build fails without them.
- **`batdoc-core`** is a git dependency (`https://github.com/daemonp/batdoc`). First build requires network access to fetch it.

## Build & Test

- **Build:** `cargo build`
- **Lint:** `cargo clippy --all-targets --all-features -- -D warnings`
- **Format check:** `cargo fmt -- --check`
- **Test:** `cargo test`
- **CI order:** `fmt → clippy → build --release → test` (see `.github/workflows/ci.yml`)
- **Run locally:** `./target/debug/rummage --maildir ./mail/test-archive --notmuch-config ./notmuch-config --port 8000`
- **Smoke test:** `curl -s http://localhost:8000/api/stats | python3 -m json.tool`
- **Logging:** Set `RUST_LOG=info` (or `debug`, `rummage=debug`, etc.) — uses `tracing` with `EnvFilter`.

## Architecture

### Key Directories

- `src/main.rs` — Entrypoint. Loads `.env`, parses CLI via `clap`, starts DB worker and server.
- `src/config.rs` — CLI flags and env vars via `clap::Parser`. All `RUMMAGE_*` env vars defined here.
- `src/server.rs` — Axum router, middleware (CSP, X-Frame-Options), HTML handlers, theme cookie parsing.
- `src/db.rs` — Single `spawn_blocking` worker thread for all `libnotmuch` access (not thread-safe). Communicates via `mpsc` + `oneshot` channels. ~800 lines; contains all DB request/response types.
- `src/error.rs` — `AppError` enum with `IntoResponse` impl. `Result<T>` alias used throughout.
- `src/api/` — JSON API handlers: `search.rs`, `thread.rs`, `attachment.rs`, `message.rs`, `tags.rs`, `stats.rs`, `senders.rs`.
- `src/ui/` — Dioxus SSR components: `mod.rs` (layout, `render_page`), `search.rs`, `thread.rs`, `home.rs`.
- `src/mail/` — Email parsing (`parser.rs`), body conversion/sanitization (`body.rs`). No attachment file here — attachment handling is in `src/api/attachment.rs` and `src/db.rs`.
- `src/mcp/` — MCP server adapter over the DB worker: `tools.rs` (tool definitions), `prompts.rs`, `resources.rs`, `util.rs`, `error.rs`, `tests.rs`.
- `src/assets.rs` — Compile-time embedded CSS/JS/SVG via `include_str!` from `assets/`.
- `contrib/` — Reference implementations from other projects (netviel, notmore). Not used at runtime.

### Critical Patterns

1. **All DB access is serialized.** `DbHandle` sends `DbRequest` variants to a single blocking worker. Never open multiple notmuch `Database` handles concurrently — libnotmuch uses `Rc<>` internally.
2. **Dioxus props must be `Clone + PartialEq`.** The `#[component]` macro requires both traits on all prop structs.
3. **No `{:,}` in `rsx!` macros.** Format-string grouping separators don't work inside Dioxus text nodes. Pre-format numbers with helper functions.
4. **Zero runtime file dependencies.** All assets are embedded via `include_str!`. CSS is concatenated and served at `/styles.css`.
5. **Theme via cookie, not localStorage.** Server needs the theme on first render. Cookie name: `theme`, values: `dark` or `light`. Default: `dark`.
6. **Accept-header branching.** `GET /search?q=…` returns HTML by default; JSON when `Accept: application/json` is present.
7. **MCP at `/mcp`.** Streamable HTTP transport via `rmcp`. Can be disabled with `--no-mcp`. The handler is a thin adapter over the same `DbHandle`.

## Coding Style

- Use `tracing` for all logging (not `println!`).
- Prefer `Result<T>` (alias in `error.rs`) over panics.
- Use `tokio::join!` for parallel independent DB queries.
- Keep handlers thin; business logic lives in `do_*` functions in `api/` or `db.rs`.
- Use `map_err(AppError::Notmuch)?` for notmuch errors.
- Use `#[component]` for all Dioxus components.

## Testing

- Unit tests in `src/mail/body.rs` (quote nesting, URL linking, HTML escaping), `src/ui/mod.rs` (render_page, theme), `src/mcp/util.rs` and `src/mcp/tests.rs` (MCP tool logic).
- Integration tests use the real maildir at `mail/test-archive/`; skip if absent.
- All tests run with `cargo test` — no special setup beyond the system library.

## Common Pitfalls

- **Relative maildir paths fail.** The notmuch crate requires absolute paths. Use `std::fs::canonicalize` if accepting user input.
- **`notmuch-config` has an absolute path.** The checked-in `notmuch-config` contains a machine-specific absolute `database.path`. Adjust it if running on a different machine.
- **Port conflicts.** The test maildir has ~5.7k messages; indexing takes ~8s on first run. Subsequent starts are sub-second.
- **Stale server processes.** Smoke tests that background the server can leave processes on ports.
- **Dioxus SSR renders to String.** `dioxus_ssr::render_element(rsx! { ... })` returns raw HTML. `render_page()` wraps it in the full document shell.
