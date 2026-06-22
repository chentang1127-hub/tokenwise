//! TokenWise Core — A self-hosted execution layer for LLM applications.
//!
//! Architecture:
//!   Your App ──→ Proxy (:9401) ──→ Smart Router ──→ AI APIs
//!                       │
//!                       └── Admin Dashboard (:9400)

use std::net::SocketAddr;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use tokenwise::config::Config;
use tokenwise::webhooks::WebhookDispatcher;
use tokenwise::{admin, license, proxy, recording};

/// TokenWise Core — Self-hosted LLM execution layer. One binary, zero code changes.
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
    /// Import Claude Code API call history from JSONL transcript files
    Import {
        /// Directory containing JSONL transcript files (recursive scan)
        #[arg(short, long, default_value = ".claude/projects")]
        source: String,
    },
    /// Generate a Pro license key (valid for 365 days by default)
    Keygen {
        /// Days until expiry (default 365)
        #[arg(short, long, default_value = "365")]
        days: u64,
    },
    /// Backup the SQLite database (WAL checkpoint + copy to output dir)
    Backup {
        /// Output directory (default: current directory)
        #[arg(short, long, default_value = ".")]
        output: String,
    },
    /// Show running status (checks if proxy and dashboard are reachable)
    Status,
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
    if let Some(ref market) = cli.market
        && (market == "cn" || market == "zh")
    {
        cli.config = "config.cn.yaml".to_string();
    }

    // Load config
    let mut cfg = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config: {e}");
            std::process::exit(1);
        }
    };

    // Apply TW_* environment variable overrides
    cfg.apply_env_overrides();

    // Verify license
    let license_tier = license::verify_license(&cfg.license.key);

    // Enforce Free tier restrictions
    if license_tier == license::LicenseTier::Free && cfg.safety_net.enabled {
        info!("Free tier: safety net disabled (Pro feature)");
        cfg.safety_net.enabled = false;
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
        }
        Command::Keygen { days } => {
            use base64::Engine;
            use hmac::{Hmac, Mac};
            use sha2::Sha256;

            const SECRET: &[u8] = &[
                0x7f, 0xa2, 0xd1, 0x3e, 0x8b, 0x55, 0x91, 0xc4, 0xf0, 0x6d, 0x2a, 0x79, 0x0e, 0xb8,
                0x33, 0x5c, 0xa1, 0x94, 0xe7, 0x2f, 0x46, 0xd8, 0x0b, 0xc6, 0x1a, 0x3d, 0x57, 0x9f,
                0xe2, 0x04, 0x68, 0xcd,
            ];

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let expires_at = now + days * 86400;
            let expiry_bytes = expires_at.to_be_bytes();
            let mut mac = Hmac::<Sha256>::new_from_slice(SECRET).unwrap();
            mac.update(&expiry_bytes);
            let signature = mac.finalize().into_bytes();
            let mut payload = Vec::with_capacity(40);
            payload.extend_from_slice(&expiry_bytes);
            payload.extend_from_slice(&signature);
            let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&payload);
            println!("tw_pro_{}", encoded);
            println!(
                "Expires: {} ({} days from now)",
                {
                    let dt = chrono::DateTime::from_timestamp(expires_at as i64, 0);
                    dt.map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default()
                },
                days
            );
        }
        Command::Backup { output } => {
            info!("Backing up database from: {}", cfg.storage.db_path);

            // Open the store (this runs WAL checkpoint on open)
            let store =
                recording::Store::new(&cfg.storage.db_path).expect("Failed to open SQLite store");
            store.checkpoint();

            // Build output filename with timestamp
            let now = chrono::Local::now();
            let ts = now.format("%Y%m%d_%H%M%S");
            let src_path = std::path::Path::new(&cfg.storage.db_path);
            let fname = src_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "tokenwise.db".to_string());
            let backup_name = format!("{}.{}.bak", fname, ts);
            let output_path = std::path::Path::new(&output).join(&backup_name);

            // Create output directory if needed
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                    error!("Failed to create output directory: {e}");
                    std::process::exit(1);
                });
            }

            // Copy the database file
            std::fs::copy(&cfg.storage.db_path, &output_path).unwrap_or_else(|e| {
                error!("Failed to copy database: {e}");
                std::process::exit(1);
            });

            let size = std::fs::metadata(&output_path)
                .map(|m| m.len())
                .unwrap_or(0);
            info!(
                "✅ Backup complete: {} ({:.1} KB)",
                output_path.display(),
                size as f64 / 1024.0
            );
        }
        Command::Status => {
            let proxy_parts: Vec<&str> = cfg.proxy.listen.split(':').collect();
            let admin_parts: Vec<&str> = cfg.proxy.admin.split(':').collect();

            let proxy_port = proxy_parts.last().copied().unwrap_or("9401");
            let admin_port = admin_parts.last().copied().unwrap_or("9400");

            // Check if the process is running by connecting to the health endpoint
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .expect("Failed to build HTTP client");
            let admin_url = format!("http://127.0.0.1:{}/health", admin_port);
            let proxy_url = format!("http://127.0.0.1:{}/health", proxy_port);

            let admin_ok = client
                .get(&admin_url)
                .send()
                .map(|r| r.status().is_success())
                .unwrap_or(false);
            let proxy_ok = client
                .get(&proxy_url)
                .send()
                .map(|r| r.status().is_success())
                .unwrap_or(false);

            println!("TokenWise Core v{}", env!("CARGO_PKG_VERSION"));
            println!(
                "  Admin Dashboard ({}):  {}",
                cfg.proxy.admin,
                if admin_ok {
                    "✅ running"
                } else {
                    "❌ not reachable"
                }
            );
            println!(
                "  Proxy           ({}):  {}",
                cfg.proxy.listen,
                if proxy_ok {
                    "✅ running"
                } else {
                    "❌ not reachable"
                }
            );
            println!("  Database:               {}", cfg.storage.db_path);
            if std::path::Path::new(&cfg.storage.db_path).exists() {
                let meta = std::fs::metadata(&cfg.storage.db_path).unwrap();
                println!("    Size: {:.1} KB", meta.len() as f64 / 1024.0);
            }
            println!("  License tier:           {}", license_tier.name());
            println!("  Locale:                 {}", cfg.locale);
            println!("  Headless:               {}", cfg.headless);

            if !admin_ok && !proxy_ok {
                println!("\nTokenWise does not appear to be running. Start it with:");
                println!("  tokenwise start");
                std::process::exit(1);
            }
        }
        Command::Import { source } => {
            info!("Importing Claude Code history from: {source}");

            // Open the store (may be different DB than default)
            let store = recording::Store::new(&cfg.storage.db_path)
                .expect("Failed to initialize SQLite store");

            let source_path = std::path::Path::new(&source);
            if !source_path.exists() {
                error!("Source directory not found: {source}");
                std::process::exit(1);
            }

            match tokenwise::import::import_from_directory(source_path, &store, &cfg) {
                Ok(result) => {
                    info!("Files scanned:    {}", result.files_scanned);
                    info!("Lines parsed:     {}", result.lines_parsed);
                    info!("Messages found:   {}", result.messages_found);
                    info!("Records inserted: {}", result.records_inserted);
                    info!("Total cost:       ${:.4}", result.total_cost);
                }
                Err(e) => {
                    error!("Import failed: {e}");
                    std::process::exit(1);
                }
            }

            store.checkpoint();
            info!("Import complete. Data saved to {}", cfg.storage.db_path);
        }
        Command::Start => {
            info!("🚀 TokenWise Core starting...");
            info!("   Proxy: http://{}", cfg.proxy.listen);
            info!("   Dashboard: http://{}", cfg.proxy.admin);
            info!("   License: {} tier", license_tier.name());
            if !license_tier.routing_enabled() {
                info!(
                    "   💡 Free tier: pass-through mode — no smart routing. \
                     Pro saves 70-90% on API calls."
                );
            }

            // Initialize recording store
            let store = recording::Store::new(&cfg.storage.db_path)
                .expect("Failed to initialize SQLite store");
            let store = Arc::new(store);

            // Build shared app state
            let routing_enabled = license_tier.routing_enabled();
            let metrics = Arc::new(admin::Metrics::default());
            let start_time = std::time::Instant::now();
            let state = Arc::new(admin::AppState {
                config: cfg.clone(),
                store: store.clone(),
                routing_enabled,
                config_path: cli.config.clone(),
                metrics: metrics.clone(),
                start_time,
            });

            // Spawn the admin dashboard on its own task
            let admin_cfg = cfg.clone();
            let admin_state = state.clone();
            let admin_addr: SocketAddr = admin_cfg
                .proxy
                .admin
                .parse()
                .expect("Invalid admin listen address");

            // Oneshot channel to signal admin server is bound
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
            let is_first = cfg.is_first_run();

            tokio::spawn(async move {
                let app = admin::build_router(admin_state);
                let listener = TcpListener::bind(admin_addr).await.unwrap();
                info!("📊 Dashboard listening on http://{}", admin_addr);
                // Signal readiness so the browser can be opened
                let _ = ready_tx.send(());
                axum::serve(listener, app).await.unwrap();
            });

            // On first run (no API keys), wait for admin to bind then open browser
            // Skip in headless mode (Docker/CI/remote servers)
            if is_first && !cfg.headless {
                let _ = ready_rx.await;
                let setup_url = format!("http://{}/setup", admin_cfg.proxy.admin);
                info!("🆕 First run detected — opening setup wizard at {setup_url}");
                if let Err(e) = open::that(&setup_url) {
                    info!("Could not open browser automatically: {e}");
                    info!("Please open {setup_url} manually.");
                }
            }

            // Run the proxy on the main task
            let proxy_addr: SocketAddr = cfg
                .proxy
                .listen
                .parse()
                .expect("Invalid proxy listen address");
            let listener = TcpListener::bind(proxy_addr).await.unwrap();
            info!("🔀 Proxy listening on http://{}", proxy_addr);

            // Build webhook dispatcher if URL is configured
            let webhook = WebhookDispatcher::new(cfg.webhook.clone())
                .map(|d| Arc::new(tokio::sync::Mutex::new(d)));

            // Spawn periodic budget check + daily usage report (every 5 minutes)
            if let Some(ref webhook_arc) = webhook {
                let periodic_store = store.clone();
                let periodic_cfg = cfg.clone();
                let periodic_webhook = webhook_arc.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                    loop {
                        interval.tick().await;
                        let now_ts = chrono::Utc::now().timestamp();
                        let today_start = chrono::Utc::now()
                            .date_naive()
                            .and_hms_opt(0, 0, 0)
                            .unwrap()
                            .and_utc()
                            .timestamp();
                        let spent_today = periodic_store.total_cost_since(today_start);
                        let month_start = chrono::Utc::now().format("%Y-%m-01").to_string();
                        let month_start_ts =
                            chrono::NaiveDate::parse_from_str(&month_start, "%Y-%m-%d")
                                .unwrap()
                                .and_hms_opt(0, 0, 0)
                                .unwrap()
                                .and_utc()
                                .timestamp();
                        let spent_month = periodic_store.total_cost_since(month_start_ts);

                        // Budget alerts (warning / exceeded with cooldown)
                        let mut dispatcher = periodic_webhook.lock().await;
                        dispatcher
                            .check_budget(
                                spent_today,
                                periodic_cfg.budget.daily_limit_usd,
                                spent_month,
                                periodic_cfg.budget.monthly_limit_usd,
                                now_ts,
                            )
                            .await;

                        // Daily usage report
                        let monthly_stats = periodic_store.monthly_stats(None).unwrap_or_default();
                        let cache_stats = periodic_store.cache_stats();
                        let routing_count = periodic_store.routing_count(None);
                        dispatcher
                            .send_usage_report(
                                now_ts,
                                monthly_stats.total_calls,
                                monthly_stats.total_cost,
                                monthly_stats.total_prompt_tokens,
                                monthly_stats.total_completion_tokens,
                                cache_stats
                                    .total_hits
                                    .saturating_sub(cache_stats.total_entries)
                                    .max(0),
                                routing_count,
                            )
                            .await;
                    }
                });
            }

            let proxy_service = proxy::build_service(
                cfg,
                store.clone(),
                routing_enabled,
                metrics.clone(),
                webhook,
            );

            // Graceful shutdown: checkpoint WAL on Ctrl+C
            let shutdown_store = store.clone();
            let accept_loop = async {
                loop {
                    let (stream, _) = match listener.accept().await {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Accept error: {e}");
                            break;
                        }
                    };
                    let svc = proxy_service.clone();
                    tokio::spawn(async move {
                        if let Err(e) = hyper::server::conn::http1::Builder::new()
                            .serve_connection(hyper_util::rt::TokioIo::new(stream), svc)
                            .await
                            && !e.to_string().contains("connection reset")
                        {
                            error!("Proxy connection error: {e}");
                        }
                    });
                }
            };

            tokio::select! {
                _ = accept_loop => {},
                _ = tokio::signal::ctrl_c() => {
                    info!("🛑 Shutting down...");
                    shutdown_store.checkpoint();
                    info!("✅ WAL checkpointed — all data saved.");
                }
            }
        }
    }
}
