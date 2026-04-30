use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "rummage",
    about = "Email archive search server powered by notmuch"
)]
pub struct Config {
    /// Path to the maildir directory
    #[arg(short, long, env = "RUMMAGE_MAILDIR")]
    pub maildir: PathBuf,

    /// Path to a notmuch config file (optional)
    #[arg(short, long, env = "RUMMAGE_NOTMUCH_CONFIG")]
    pub notmuch_config: Option<PathBuf>,

    /// Force a full re-index and exit
    #[arg(long)]
    pub index: bool,

    /// Skip auto-initialization of the notmuch database on first run
    #[arg(long)]
    pub no_auto_index: bool,

    /// Host to bind to (default: 127.0.0.1)
    #[arg(short = 'H', long, default_value = "127.0.0.1", env = "RUMMAGE_HOST")]
    pub host: String,

    /// Port to listen on
    #[arg(short, long, default_value_t = 8000, env = "RUMMAGE_PORT")]
    pub port: u16,

    /// Disable HTML routes and static assets
    #[arg(long, env = "RUMMAGE_NO_WEBUI")]
    pub no_webui: bool,

    /// Disable MCP transport
    #[arg(long, env = "RUMMAGE_NO_MCP")]
    pub no_mcp: bool,

    /// Custom mount point for MCP transport
    #[arg(long, default_value = "/mcp", env = "RUMMAGE_MCP_PATH")]
    pub mcp_path: String,

    /// Allowed Host headers for MCP DNS rebinding protection (default: localhost,127.0.0.1,::1)
    #[arg(
        long,
        env = "RUMMAGE_MCP_ALLOWED_HOSTS",
        value_delimiter = ',',
        default_value = "localhost,127.0.0.1,::1"
    )]
    pub mcp_allowed_hosts: Vec<String>,
}
