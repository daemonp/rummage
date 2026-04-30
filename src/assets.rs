//! Embedded static assets (CSS, JS, SVG) compiled into the binary.
//!
//! All assets are served from memory — no runtime file dependencies.

/// Shared design tokens + utility classes.
pub static TOKENS_CSS: &str = include_str!("../assets/tokens.css");

/// Instrument theme layout styles.
pub static INSTRUMENT_CSS: &str = include_str!("../assets/styles-directions.css");

/// Client-side JavaScript bundle for interactivity.
pub static APP_JS: &str = include_str!("../assets/app.js");

/// SVG favicon embedded at compile time.
pub static FAVICON_SVG: &str = include_str!("../assets/favicon.svg");
