# Rummage

A single static-binary HTTP server for browsing and searching email archives indexed by [notmuch](https://notmuchmail.org/).

Rummage is the archive reader we wished we had. After years of relying on [netviel](https://github.com/djcb/netviel) (the excellent Python/SPA frontend shipped with notmuch) and the classic PHP-based `notmore`, we wanted something that combined the best of both worlds — a polished, keyboard-driven web UI *and* a clean programmatic API — all in one self-contained binary with zero runtime file dependencies.

The result is a fast, local-first archive browser built with Rust, Dioxus SSR, and Axum. Every asset is embedded at compile time. The UI works without JavaScript and is enhanced by it: vim-like navigation, a command palette, inline attachment previews, and a dark/light theme that persists via cookie so the server renders correctly on first load.

**MCP (Model Context Protocol) integration** is a first-class citizen. Connect Claude Desktop, Claude Code, or any MCP client to `http://localhost:8000/mcp` and explore your archive conversationally. The AI can search threads, read messages in plain text or markdown, extract text from PDF/DOCX/XLSX attachments, and summarize conversations — all without leaving the terminal.

---

## Features

- **Single static binary** — all CSS, JS, icons, and templates are embedded via `include_str!`. No runtime file dependencies.
- **Keyboard-driven UI** — `j`/`k` navigate threads, `o` opens, `/` searches, `?` for help, `⌘K` or `Ctrl+K` for the command palette.
- **Polished three-column layout** — tags/senders sidebar, thread list, and a reader pane with inline attachment previews (PDF, images, video, text).
- **Safe email rendering** — HTML bodies are sanitized with `ammonia`; plain-text bodies are converted to HTML with nested `<blockquote>` support for `>` quotes and auto-linked URLs.
- **Native `libnotmuch` integration** — direct Rust bindings, no subprocess spawning. Queries return in <50ms on archives with 5,000+ messages.
- **Auto-initialization** — point Rummage at a maildir and it creates and indexes the notmuch database on first run.
- **Dark / light theme** — cookie-based so the server knows your preference before the first byte reaches the browser.
- **JSON REST API** — clean, documented endpoints for search, threads, attachments, tags, and stats.
- **MCP server** — AI-assisted archive exploration via the Model Context Protocol. Search, read, summarize, and extract document text from attachments conversationally.

---

## Quick Start

### Prerequisites

- Rust toolchain (stable, 1.75+)
- `libnotmuch` development library
  - Debian/Ubuntu: `apt-get install libnotmuch-dev`
  - Fedora: `dnf install notmuch-devel`
  - Arch: `pacman -S notmuch`

### Build

```bash
git clone https://github.com/daemonp/rummage.git
cd rummage
cargo build --release
```

The binary is at `./target/release/rummage`.

### Run

Point it at a maildir. Rummage auto-creates and indexes the notmuch database on first run:

```bash
# Auto-create and index on first run
./target/release/rummage --maildir ~/Mail/Archive --notmuch-config ~/.notmuch-config

# Force a full re-index and exit
./target/release/rummage --maildir ~/Mail/Archive --index

# Skip auto-initialization (fail if DB missing)
./target/release/rummage --maildir ~/Mail/Archive --no-auto-index

# Bind to a different host/port
./target/release/rummage --maildir ~/Mail/Archive --host 0.0.0.0 --port 3000
```

Then open [http://127.0.0.1:8000](http://127.0.0.1:8000).

### Docker

The official image is built on Alpine Linux for a minimal footprint (~50 MB).
**No notmuch config file is required** — rummage auto-creates and indexes the database on first run.

```bash
# Build locally
docker build -t rummage .

# Simplest run — just mount your maildir
docker run -p 8000:8000 \
  -v ~/Mail/Archive:/mail \
  rummage

# With an existing notmuch config (optional — for custom tags/hooks)
docker run -p 8000:8000 \
  -v ~/Mail/Archive:/mail \
  -v ~/.notmuch:/notmuch \
  -e NOTMUCH_CONFIG=/notmuch/notmuch-config \
  rummage

# Full production settings
docker run -d --name rummage -p 8000:8000 \
  -v /data/email:/mail \
  -v $(pwd)/notmuch-config:/notmuch:ro \
  -e RUMMAGE_MAILDIR=/mail \
  -e RUMMAGE_HOST=0.0.0.0 \
  -e RUMMAGE_PORT=8000 \
  -e NOTMUCH_CONFIG=/notmuch/notmuch-config \
  -e RUMMAGE_MCP_ALLOWED_HOSTS=email-archive.myawesomehouse.com \
  ghcr.io/daemonp/rummage:master
```

**Volumes:**
| Path | Purpose | Required |
|---|---|---|
| `/mail` | Your maildir archive. The notmuch database is auto-created inside it at `/mail/.notmuch/`. | **Yes** |
| `/notmuch` | Optional directory for a custom `notmuch-config` file. | No |

**Environment variables:**
| Variable | Default | Description |
|---|---|---|
| `RUMMAGE_MAILDIR` | `/mail` | Path to the maildir (set via image CMD) |
| `RUMMAGE_HOST` | `0.0.0.0` | Bind address |
| `RUMMAGE_PORT` | `8000` | Listen port |
| `NOTMUCH_CONFIG` | `/notmuch/notmuch-config` | Standard notmuch config path (optional) |
| `RUMMAGE_NOTMUCH_CONFIG` | — | Rummage-specific override (optional) |
| `RUMMAGE_MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | Allowed `Host` headers for MCP DNS rebinding protection |

> **Note:** Notmuch stores its database inside the maildir tree (`maildir/.notmuch/`).  There is no separate `database_path` option — this is a notmuch convention.  If you mount the maildir read-only, auto-initialization will fail; either mount it read-write or pre-build the index locally.

---

## Web UI

Rummage serves server-rendered HTML with lightweight JavaScript enhancements. The UI works fully without JavaScript and is progressively enhanced for speed.

### Keyboard Shortcuts

| Key | Action |
|---|---|
| `j` / `k` | Next / previous thread |
| `o` or `Enter` | Open selected thread in reader pane |
| `↑` / `↓` | Navigate within search results |
| `/` | Focus search box |
| `?` | Show keyboard shortcuts help |
| `u` | Open raw `.eml` of selected thread |
| `⌘K` or `Ctrl+K` | Open command palette |

### Command Palette

Press `⌘K` (Mac) or `Ctrl+K` (Linux/Windows) to open the command palette. Search for operators, jump to saved queries, toggle the theme, or navigate directly to help.

---

## JSON API

All JSON endpoints return `Content-Type: application/json`. The same search can also be performed via `GET /search?q=…` with `Accept: application/json`.

| Method | Path | Description | Query Params |
|---|---|---|---|
| GET | `/api/search` | Search threads | `q` (required) |
| GET | `/api/thread/:id` | Get messages in a thread | none |
| GET | `/api/attachment` | Download attachment | `msg`, `part` |
| GET | `/api/message/:id` | Download raw `.eml` | none |
| GET | `/api/tags` | List all tags with counts | none |
| GET | `/api/stats` | Archive-wide statistics | none |

Example:

```bash
curl -H "Accept: application/json" "http://localhost:8000/search?q=from:alice%20since:2024-01-01"
```

---

## MCP — AI-Assisted Archive Exploration

Rummage exposes a [Model Context Protocol](https://modelcontextprotocol.io/) server at `/mcp` (Streamable HTTP transport). Connect your MCP client and explore your archive conversationally.

### What the MCP can do

- **Search** — run notmuch queries via natural language or raw syntax
- **Read threads** — fetch full conversations in plain text, markdown, or HTML
- **Summarize** — ask the AI to summarize a thread, list action items, or identify key decisions
- **Extract attachments** — pull text from PDF, DOCX, XLSX, PPTX, and legacy DOC/XLS files (via [batdoc](https://github.com/daemonp/batdoc))
- **Browse by tags and senders** — discover what’s in the archive before diving deep
- **Find related threads** — discover conversations connected by participants, subject, or tags

### Configuration

Add to your MCP client (e.g., Claude Desktop):

```json
{
  "mcpServers": {
    "rummage": {
      "url": "http://localhost:8000/mcp"
    }
  }
}
```

The MCP server is read-only and runs locally by default. No credentials, no cloud, no data leaves your machine.

---

## Configuration

Rummage reads configuration from command-line flags and environment variables. A `.env` file in the working directory is automatically loaded if present.

| Flag | Environment Variable | Default | Description |
|---|---|---|---|
| `--maildir` | `RUMMAGE_MAILDIR` | — | Path to the maildir archive |
| `--notmuch-config` | `RUMMAGE_NOTMUCH_CONFIG` | — | Path to notmuch config file |
| `--port` | `RUMMAGE_PORT` | `8000` | HTTP server port |
| `--host` | `RUMMAGE_HOST` | `127.0.0.1` | HTTP server bind address |
| `--index` | — | — | Force re-index and exit |
| `--no-auto-index` | — | — | Skip auto-initialization |
| `--no-webui` | — | false | Disable HTML routes and static assets |
| `--no-mcp` | — | false | Disable MCP transport at `/mcp` |
| `--mcp-allowed-hosts` | `RUMMAGE_MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | Comma-separated allowed Host headers for MCP (reverse-proxy safety) |

---

## Architecture

- **Dioxus SSR** — server-rendered HTML components written in Rust. The same components can be compiled to native/desktop later.
- **Axum** — lightweight async HTTP server with middleware for security headers (CSP, X-Frame-Options).
- **Single blocking DB worker** — `libnotmuch` is not thread-safe (it uses `Rc<>` internally). All database access is serialized through a dedicated `spawn_blocking` worker thread that receives requests via an `mpsc` channel.
- **mail-parser** — pure-Rust MIME parsing for body extraction and attachment handling.
- **ammonia** — HTML sanitization. Strips `<script>`, event handlers, and dangerous attributes.
- **linkify** — auto-linking URLs and email addresses in plain-text bodies.
- **batdoc-core** — pure-Rust document text extraction for MCP attachment reading.

---

## Acknowledgements

Rummage stands on the shoulders of two excellent projects that have served the notmuch community for years:

- **[netviel](https://github.com/djcb/netviel)** — the Python/SPA frontend that ships with notmuch. Its clean API and fast search set the standard for archive browsing. We used it for years and learned a great deal from it.
- **notmore** — the classic PHP server-rendered interface that proved email archives can be browsed simply and effectively without heavy client-side frameworks.

We combined the best ideas from both: the speed and API of netviel with the simplicity and server-rendered reliability of notmore, then added a layer of modern Rust tooling and AI accessibility via MCP.

---

## License

MIT
