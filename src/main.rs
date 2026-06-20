//! TokenWise — A Rust transparent proxy that saves 70-90% on AI API costs.
//!
//! Architecture:
//!   Your App ──→ Proxy (:9401) ──→ Smart Router ──→ AI APIs
//!                       │
//!                       └── Admin Dashboard (:9400)

mod admin;
mod config;
mod cost;
mod license;
mod proxy;
mod recording;

use std::net::SocketAddr;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use config::Config;

/// TokenWise — Save 70-90% on AI API costs. One binary, zero code changes.
#[derive(Parser)]
#[command(name = "tokenwise", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to config file
    #[arg(short, long, default_value = "config.yaml")]
    config: String,

    /// Target market: global (Western) or cn (China). Shorthand for --config.
    #[arg(short, long)]
    market: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the proxy and admin dashboard
    Start,
    /// Validate config file
    Validate,
}

#[tokio::main]
async fn main() {
    // Init logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tokenwise=debug")),
        )
        .init();

    let mut cli = Cli::parse();

    // --market cn is shorthand for --config config.cn.yaml
    if let Some(ref market) = cli.market {
        if market == "cn" || market == "zh" {
            cli.config = "config.cn.yaml".to_string();
        }
    }

    // Load config
    let mut cfg = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config: {e}");
            std::process::exit(1);
        }
    };

    // Verify license
    let license_tier = license::verify_license(&cfg.license.key);

    // Enforce Free tier restrictions
    if license_tier == license::LicenseTier::Free {
        if cfg.safety_net.enabled {
            info!("Free tier: safety net disabled (Pro feature)");
            cfg.safety_net.enabled = false;
        }
        if let Some(max) = license_tier.max_providers() {
            if cfg.providers.len() > max {
                warn!(
                    "Free tier limited to {max} providers. Truncating from {} to {max}.",
                    cfg.providers.len()
                );
                cfg.providers.truncate(max);
            }
        }
    }

    let cfg = Arc::new(cfg);

    match cli.command.unwrap_or(Command::Start) {
        Command::Validate => {
            info!("Config is valid.");
            info!(
                "Proxy: {} | Admin: {} | Providers: {} | License: {}",
                cfg.proxy.listen,
                cfg.proxy.admin,
                cfg.providers.len(),
                license_tier.name(),
            );
            if license_tier == license::LicenseTier::Free && cfg.providers.len() > 3 {
                warn!(
                    "Free tier limited to 3 providers, but {} configured. \
                     Only the first 3 will be used.",
                    cfg.providers.len()
                );
            }
        }
        Command::Start => {
            info!("🚀 TokenWise starting...");
            info!("   Proxy: http://{}", cfg.proxy.listen);
            info!("   Dashboard: http://{}", cfg.proxy.admin);
            info!("   License: {} tier", license_tier.name());

            // Initialize recording store
            let store = recording::Store::new(&cfg.storage.db_path)
                .expect("Failed to initialize SQLite store");
            let store = Arc::new(store);

            // Build shared app state
            let state = Arc::new(admin::AppState {
                config: cfg.clone(),
                store: store.clone(),
            });

            // Spawn the admin dashboard on its own task
            let admin_cfg = cfg.clone();
            let admin_state = state.clone();
            let admin_addr: SocketAddr = admin_cfg
                .proxy
                .admin
                .parse()
                .expect("Invalid admin listen address");

            tokio::spawn(async move {
                let app = admin::build_router(admin_state);
                let listener = TcpListener::bind(admin_addr).await.unwrap();
                info!("📊 Dashboard listening on http://{}", admin_addr);
                axum::serve(listener, app).await.unwrap();
            });

            // Run the proxy on the main task
            let proxy_addr: SocketAddr = cfg
                .proxy
                .listen
                .parse()
                .expect("Invalid proxy listen address");
            let listener = TcpListener::bind(proxy_addr).await.unwrap();
            info!("🔀 Proxy listening on http://{}", proxy_addr);

            let proxy_service = proxy::build_service(cfg, store);

            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let svc = proxy_service.clone();
                tokio::spawn(async move {
                    if let Err(e) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
                        .await
                    {
                        if !e.to_string().contains("connection reset") {
                            error!("Proxy connection error: {e}");
                        }
                    }
                });
            }
        }
    }
}
