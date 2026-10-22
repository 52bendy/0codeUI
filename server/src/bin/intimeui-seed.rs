//! 种子数据一键加载 CLI。
//!
//! 用法：
//! ```bash
//! cargo run -p intimeui-server --bin intimeui-seed
//! cargo run -p intimeui-server --bin intimeui-seed -- --database-url sqlite://data/intimeui.db
//! ```
//!
//! 命令会先执行幂等迁移，再加载幂等种子数据；重复执行不会产生重复记录。

use std::process::ExitCode;

use intimeui_server::config::AppConfig;
use intimeui_server::db::Db;

#[tokio::main]
async fn main() -> ExitCode {
    let _ = dotenvy::dotenv();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "Usage: intimeui-seed [--database-url <URL>]\n\
             \n\
             Loads idempotent seed data (PRD §5.1: usr_9527 / ORD_20261014_001).\n\
             Defaults to DATABASE_URL from the environment, or sqlite://data/intimeui.db."
        );
        return ExitCode::SUCCESS;
    }

    let database_url = match parse_database_url(&args) {
        Ok(Some(url)) => url,
        Ok(None) => match AppConfig::from_env() {
            Ok(config) => config.database_url,
            Err(error) => {
                eprintln!("configuration error: {error}");
                return ExitCode::FAILURE;
            }
        },
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    let db = match Db::connect(&database_url).await {
        Ok(db) => db,
        Err(error) => {
            eprintln!("failed to connect database `{database_url}`: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = db.migrate().await {
        eprintln!("failed to run migrations: {error}");
        return ExitCode::FAILURE;
    }

    match db.seed().await {
        Ok(report) => {
            println!(
                "seed completed: users={}, products={}, orders={}, order_items={}, \
                 operation_records={}, total={}",
                report.users,
                report.products,
                report.orders,
                report.order_items,
                report.operation_records,
                report.total()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("failed to load seed data: {error}");
            ExitCode::FAILURE
        }
    }
}

/// 解析 `--database-url <URL>`。
fn parse_database_url(args: &[String]) -> Result<Option<String>, String> {
    let Some(arg) = args.first().map(String::as_str) else {
        return Ok(None);
    };

    if arg == "--database-url" {
        return args
            .get(1)
            .cloned()
            .map(Some)
            .ok_or_else(|| "--database-url requires a value".to_string());
    }

    if let Some(url) = arg.strip_prefix("--database-url=") {
        return Ok(Some(url.to_string()));
    }

    Err(format!("unknown argument `{arg}` (try --help)"))
}
