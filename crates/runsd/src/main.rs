use std::{io, path::PathBuf, sync::Arc};

use anyhow::Context;
use chrono::Utc;
use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};
use sqlx::sqlite::SqlitePoolOptions;
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use runsd::{
    actor::{
        db as db_actor,
        event_bus::EventBus,
        supervisor::Supervisor,
        watchdog,
        worker_pool::WorkerPool,
    },
    api::{routes, state::AppState},
    config::Config,
    db::queries,
};

#[derive(Parser, Debug)]
#[command(name = "runsd", about = "Run calculation daemon")]
struct Cli {
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Print the default configuration as TOML to stdout and exit.
    #[arg(long)]
    init_config: bool,

    /// Print shell completions for the given shell and exit.
    #[arg(long, value_name = "SHELL")]
    completions: Option<Shell>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        generate(shell, &mut Cli::command(), "runsd", &mut io::stdout());
        return Ok(());
    }

    if cli.init_config {
        let default = Config::default();
        print!("{}", toml::to_string_pretty(&default).expect("serialize config"));
        return Ok(());
    }

    let config = Config::load(cli.config.as_deref()).context("failed to load configuration")?;

    // Create directories before setup_tracing opens the log file.
    if let Some(p) = config.logging.file_path.parent() {
        std::fs::create_dir_all(p)
            .with_context(|| format!("failed to create log dir {}", p.display()))?;
    }
    std::fs::create_dir_all(&config.server.data_dir)
        .with_context(|| format!("failed to create data dir {}", config.server.data_dir.display()))?;
    #[cfg(unix)]
    if let Some(p) = config.server.socket_path.parent() {
        std::fs::create_dir_all(p)
            .with_context(|| format!("failed to create socket dir {}", p.display()))?;
    }

    setup_tracing(&config)?;
    info!("runsd starting");

    // Database pools.
    let db_path = config.server.data_dir.join("runsd.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());

    let write_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&db_url)
        .await
        .context("failed to open SQLite (write)")?;

    let read_pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect(&db_url)
        .await
        .context("failed to open SQLite (read)")?;

    // Apply connection-level PRAGMAs outside any transaction.
    sqlx::query("PRAGMA journal_mode = WAL").execute(&write_pool).await?;
    sqlx::query("PRAGMA synchronous = NORMAL").execute(&write_pool).await?;
    sqlx::query("PRAGMA journal_mode = WAL").execute(&read_pool).await?;
    sqlx::query("PRAGMA synchronous = NORMAL").execute(&read_pool).await?;

    // Run migrations.
    sqlx::migrate!("./migrations")
        .run(&write_pool)
        .await
        .context("migration failed")?;

    // Crash recovery sweep.
    queries::crash_recovery_sweep(
        &write_pool,
        Utc::now().timestamp_millis(),
        config.retry.max_attempts,
    )
    .await
    .context("crash recovery sweep failed")?;

    // Actor system.
    let config = Arc::new(config);
    let bus = EventBus::new();
    let pool = WorkerPool::new(config.server.max_concurrent_calculations);
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.external_api.request_timeout_s))
        .build()?;

    let db_handle = db_actor::spawn(write_pool);

    let mut supervisor = Supervisor::new(
        db_handle.clone(),
        bus.clone(),
        pool,
        http_client,
        Arc::clone(&config),
    );
    supervisor.restore_active_runs().await?;
    let supervisor_handle = supervisor.spawn();

    // Watchdog.
    let wd_db = db_handle.clone();
    let wd_sup = supervisor_handle.clone();
    let wd_cfg = config.lease.clone();
    tokio::spawn(async move {
        watchdog::run_watchdog(wd_db, wd_sup, wd_cfg).await;
    });

    // Event pruning background task — runs hourly.
    if config.logging.event_retention_days > 0 {
        let prune_db = db_handle.clone();
        let retention_days = config.logging.event_retention_days;
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(3_600));
            loop {
                interval.tick().await;
                let cutoff_ms = Utc::now().timestamp_millis()
                    - (retention_days as i64 * 86_400_000);
                match prune_db.prune_events(cutoff_ms).await {
                    Ok(n) if n > 0 => tracing::info!(deleted = n, "pruned old events"),
                    Err(e) => tracing::warn!(error = %e, "event pruning failed"),
                    _ => {}
                }
            }
        });
    }

    let app_state = AppState {
        db: db_handle,
        read_pool,
        bus,
        supervisor: supervisor_handle.clone(),
        config: Arc::clone(&config),
    };

    let app = routes::router(app_state);

    let sup_shutdown = supervisor_handle.clone();
    let shutdown = async move {
        tokio::signal::ctrl_c().await.ok();
        info!("shutdown signal received");
        sup_shutdown.shutdown().await;
    };

    #[cfg(unix)]
    {
        let sock_path = config.server.socket_path.clone();
        if sock_path.exists() {
            tokio::fs::remove_file(&sock_path).await.ok();
        }
        let listener = UnixListener::bind(&sock_path)
            .with_context(|| format!("failed to bind UDS at {}", sock_path.display()))?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600))?;
        info!(socket = %sock_path.display(), "API listening");
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
            .context("axum server error")?;
    }

    #[cfg(windows)]
    {
        let addr = format!("127.0.0.1:{}", config.server.port);
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("failed to bind TCP at {addr}"))?;
        info!(address = %addr, "API listening");
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
            .context("axum server error")?;
    }

    Ok(())
}

fn setup_tracing(config: &Config) -> anyhow::Result<()> {
    let stderr_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.logging.stderr_level.as_str()));

    let log_dir = config
        .logging
        .file_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(std::env::temp_dir);

    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::NEVER)
        .filename_prefix("runsd")
        .filename_suffix("log")
        .max_log_files(7)
        .build(log_dir)?;

    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    Box::leak(Box::new(guard)); // Keep flush guard alive for the process lifetime.

    // Use the file_level filter for the file writer, stderr_filter for stderr.
    // We compose both layers under a single registry.
    use tracing_subscriber::Layer;
    let file_filter = EnvFilter::new(config.logging.file_level.as_str());
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr).with_filter(stderr_filter))
        .with(fmt::layer().json().with_writer(non_blocking).with_filter(file_filter))
        .init();

    Ok(())
}
