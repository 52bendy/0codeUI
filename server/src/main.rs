//! 服务入口：加载配置 → 初始化日志 → 组装应用 → 监听（优雅停机）。

use std::process::ExitCode;

use intimeui_server::config::AppConfig;
use intimeui_server::db::Db;
use intimeui_server::state::AppState;
use intimeui_server::{build_app, telemetry};

#[tokio::main]
async fn main() -> ExitCode {
    // 本地开发便利：自动加载当前目录 .env（生产请使用真实 Secrets 注入，不要落盘）。
    let _ = dotenvy::dotenv();

    let config = match AppConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::FAILURE;
        }
    };

    telemetry::init_telemetry(&config.log_level, config.log_format);

    let host = config.host.clone();
    let port = config.port;

    let db = match Db::connect(&config.database_url).await {
        Ok(db) => db,
        Err(error) => {
            tracing::error!(error = %error, "failed to connect database");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = db.migrate().await {
        tracing::error!(error = %error, "failed to run database migrations");
        return ExitCode::FAILURE;
    }
    tracing::info!(database_url = %config.database_url, "database ready");

    let state = AppState::new(config, db);
    spawn_confirmation_sweeper(state.clone());
    let app = build_app(state);

    let listener = match tokio::net::TcpListener::bind((host.as_str(), port)).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(error = %error, "failed to bind {}:{}", host, port);
            return ExitCode::FAILURE;
        }
    };

    let addr = listener
        .local_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| format!("{host}:{port}"));
    tracing::info!(
        service = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
        "listening on {}",
        addr
    );

    let result = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await;

    match result {
        Ok(()) => {
            tracing::info!("server stopped gracefully");
            ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(error = %error, "server error");
            ExitCode::FAILURE
        }
    }
}

/// 后台清理过期确认单：每 30 秒最多处理 200 条。
///
/// 确认 / 拒绝路径本身也会做惰性过期检查；此任务保证无人操作的确认单
/// 也能进入 `EXPIRED` 终态并留下审计事件。
fn spawn_confirmation_sweeper(state: AppState) {
    let service = state.confirmation_service.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            match service.expire_stale(200).await {
                Ok(0) => {}
                Ok(count) => {
                    tracing::info!(count, "expired stale intent confirmations");
                }
                Err(error) => {
                    tracing::warn!(error = %error, "failed to expire stale intent confirmations");
                }
            }
        }
    });
}

/// 等待 Ctrl-C / SIGTERM，实现优雅停机。
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    tracing::info!("shutdown signal received");
}
