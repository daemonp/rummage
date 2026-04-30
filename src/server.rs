use crate::api;
use crate::config::Config;
use crate::db::DbHandle;
use crate::error::Result;
use crate::ui::home::HomePage;
use crate::ui::search::SearchPage;
use crate::ui::thread::ThreadPage;
use crate::ui::{render_page, AppShell, Header, SidebarRow, Subheader};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::header::COOKIE;
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use dioxus::prelude::*;
use serde::Deserialize;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

/// Router configuration controlling which route groups are mounted.
pub struct RouterConfig {
    pub webui_enabled: bool,
    pub mcp_enabled: bool,
    pub mcp_path: String,
}

/// Extract the theme preference from the Cookie header.
/// Returns "dark" or "light".
fn theme_from_cookies(headers: &axum::http::HeaderMap) -> &'static str {
    let cookie_hdr = headers.get(COOKIE);
    if let Some(val) = cookie_hdr {
        if let Ok(s) = val.to_str() {
            for cookie in s.split(';') {
                let mut parts = cookie.trim().splitn(2, '=');
                if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                    if name == "theme" && value == "light" {
                        return "light";
                    }
                }
            }
        }
    }
    "dark"
}

/// Simple liveness / health endpoint that returns 200 as soon as the server
/// is listening.  Does not touch the database, so it works during indexing.
async fn health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({"status": "ok"}))
}

/// Readiness probe endpoint. Returns 200 only when the database is ready
/// to serve requests; returns 503 while initialization is in progress.
async fn ready_handler(State(db): State<DbHandle>) -> impl IntoResponse {
    if db.is_ready() {
        (
            StatusCode::OK,
            axum::Json(serde_json::json!({"status": "ready"})),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({"status": "not ready"})),
        )
    }
}

/// Start the Axum HTTP server on the given host and port.
///
/// # Errors
/// Returns `anyhow::Error` if the TCP listener cannot be bound
/// or if the server encounters a fatal error while running.
pub async fn serve(
    db: DbHandle,
    config: &Config,
    router_config: RouterConfig,
) -> anyhow::Result<()> {
    let app = router(db, router_config).await;

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Listening on http://{addr}");

    axum::serve(listener, app).await?;
    Ok(())
}

/// W3C common-log-format request logging.
///
/// Logs: remote-host ident authuser [date] "method path protocol" status bytes latency
async fn request_logger(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let start = std::time::Instant::now();

    let remote_addr = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "-".to_string());

    let method = request.method().to_string();
    let path = request
        .uri()
        .path_and_query()
        .map_or_else(|| "/".to_string(), |p| p.to_string());
    let version = format!("{:?}", request.version());

    let response = next.run(request).await;

    let status = response.status().as_u16();
    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    let content_length = response
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");

    let timestamp = chrono::Local::now().format("%d/%b/%Y:%H:%M:%S %z");

    info!(
        "{} - - [{}] \"{} {} {}\" {} {} {:.1}ms",
        remote_addr, timestamp, method, path, version, status, content_length, latency_ms
    );

    response
}

async fn security_headers(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        axum::http::header::CONTENT_SECURITY_POLICY,
        axum::http::HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'",
        ),
    );
    response.headers_mut().insert(
        axum::http::header::X_FRAME_OPTIONS,
        axum::http::HeaderValue::from_static("SAMEORIGIN"),
    );
    response
}

async fn router(db: DbHandle, router_config: RouterConfig) -> Router {
    let mut router = Router::new()
        // JSON API routes — always present
        .route("/api/health", get(health_handler))
        .route("/api/ready", get(ready_handler))
        .route("/api/search", get(api::search::handler))
        .route("/api/thread/:id", get(api::thread::handler))
        .route("/api/attachment", get(api::attachment::handler))
        .route("/api/message/:id", get(api::message::handler))
        .route("/api/tags", get(api::tags::handler))
        .route("/api/stats", get(api::stats::handler))
        .route("/api/senders", get(api::senders::handler))
        .route("/api/message/:id/detail", get(api::message::detail_handler));

    if router_config.webui_enabled {
        // HTML page routes
        router = router
            .route("/", get(home_handler))
            .route("/search", get(search_handler))
            .route("/thread/:id", get(thread_handler))
            // Static assets
            .route("/styles.css", get(styles_css_handler))
            .route("/app.js", get(app_js_handler))
            .route("/favicon.svg", get(favicon_handler));
    }

    if router_config.mcp_enabled {
        use rmcp::transport::streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        };

        let db_for_mcp = db.clone();
        let mcp_handler = crate::mcp::RummageMcpHandler::new(db_for_mcp).await;
        let mcp_service = StreamableHttpService::new(
            move || Ok(mcp_handler.clone()),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );

        router = router.nest_service(&router_config.mcp_path, mcp_service);
    }

    router
        .with_state(db)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(request_logger))
}

// ── Static asset handlers ─────────────────────────────────────────
// Content is embedded at compile time and never changes for a given binary,
// so we serve with aggressive caching and avoid `.unwrap()` in handlers.

/// Immutable cache header for compile-time-embedded assets.
const CACHE_IMMUTABLE: &str = "public, max-age=31536000, immutable";

async fn styles_css_handler() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, CACHE_IMMUTABLE),
        ],
        concat!(
            include_str!("../assets/tokens.css"),
            include_str!("../assets/styles-directions.css")
        ),
    )
}

async fn app_js_handler() -> impl IntoResponse {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (axum::http::header::CACHE_CONTROL, CACHE_IMMUTABLE),
        ],
        crate::assets::APP_JS,
    )
}

async fn favicon_handler() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/svg+xml"),
            (axum::http::header::CACHE_CONTROL, CACHE_IMMUTABLE),
        ],
        crate::assets::FAVICON_SVG,
    )
}

// ── HTML page handlers ────────────────────────────────────────────

async fn home_handler(
    State(db): State<DbHandle>,
    headers: axum::http::HeaderMap,
) -> Result<Html<String>> {
    let theme = theme_from_cookies(&headers);
    let stats = db.stats().await.ok();
    let total_messages = stats.as_ref().map(|s| s.total_messages);
    let total_threads = stats.as_ref().map(|s| s.total_threads);
    Ok(Html(render_page(
        "Rummage — Email Archive Search",
        rsx! {
            AppShell {
                Header { query: String::new(), total_messages, total_threads }
                HomePage {}
            }
        },
        theme,
    )))
}

#[derive(Debug, Deserialize)]
struct WebSearchParams {
    q: Option<String>,
}

async fn search_handler(
    State(db): State<DbHandle>,
    Query(params): Query<WebSearchParams>,
    headers: axum::http::HeaderMap,
) -> Result<Response> {
    let query = params.q.unwrap_or_else(|| "tag:inbox".into());

    // Branch on Accept header: JSON API vs HTML page.
    let wants_json = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("application/json"))
        .unwrap_or(false);

    let results = db.search(query.clone(), None, None, None).await?;

    if wants_json {
        return Ok(axum::Json(results).into_response());
    }

    // HTML path: fetch sidebar data and stats in parallel, then render page.
    let theme = theme_from_cookies(&headers);
    let start = std::time::Instant::now();
    let (tags_res, senders_res, stats_res) = tokio::join!(db.tags(), db.senders(), db.stats());

    if let Err(ref e) = tags_res {
        warn!(error = %e, "failed to fetch tags for sidebar");
    }
    if let Err(ref e) = senders_res {
        warn!(error = %e, "failed to fetch senders for sidebar");
    }

    let tags: Vec<SidebarRow> = tags_res
        .unwrap_or_default()
        .into_iter()
        .map(|(name, count)| SidebarRow { name, count })
        .collect();
    let senders: Vec<SidebarRow> = senders_res
        .unwrap_or_default()
        .into_iter()
        .map(|(name, count)| SidebarRow { name, count })
        .collect();
    let stats = stats_res.ok();
    let elapsed_ms = start.elapsed().as_millis() as u64;

    let total_messages = stats.as_ref().map(|s| s.total_messages).unwrap_or(0);
    let total_threads = stats.as_ref().map(|s| s.total_threads).unwrap_or(0);

    Ok(Html(render_page(
        &format!("Search: {query} — Rummage"),
        rsx! {
            AppShell {
                stats: format!("{} threads · {} ms", results.threads.len(), elapsed_ms),
                Header {
                    query: query.clone(),
                    total_messages: Some(total_messages),
                    total_threads: Some(total_threads),
                }
                Subheader {
                    query: query.clone(),
                    thread_count: results.threads.len(),
                    message_count: None,
                    search_time_ms: Some(elapsed_ms),
                }
                SearchPage {
                    query: query.clone(),
                    results: results.clone(),
                    selected_thread: None,
                    total_messages: total_messages,
                    total_threads: total_threads,
                    tags: tags,
                    senders: senders,
                }
            }
        },
        theme,
    ))
    .into_response())
}

async fn thread_handler(
    State(db): State<DbHandle>,
    Path(thread_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Html<String>> {
    let theme = theme_from_cookies(&headers);
    let (detail, stats) = tokio::join!(db.thread(thread_id.clone()), db.stats());
    let detail = detail?;
    let stats = stats.ok();
    let total_messages = stats.as_ref().map(|s| s.total_messages);
    let total_threads = stats.as_ref().map(|s| s.total_threads);

    let subject = detail
        .messages
        .first()
        .map(|m| m.subject.clone())
        .unwrap_or_else(|| "Thread".into());

    Ok(Html(render_page(
        &format!("{subject} — Rummage"),
        rsx! {
            AppShell {
                Header {
                    query: String::new(),
                    total_messages,
                    total_threads,
                }
                ThreadPage { detail: detail.clone() }
            }
        },
        theme,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbHandle;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn test_health_handler_returns_ok() {
        let response = health_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_handler_when_db_ready() {
        let (db, _) = DbHandle::mock(false);
        let response = ready_handler(State(db)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_handler_when_db_initializing() {
        let (db, _) = DbHandle::mock(true);
        let response = ready_handler(State(db)).await.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
