//! 数据访问基础设施：SQLite 连接池、迁移与种子数据加载。
//!
//! - 迁移脚本位于 `server/migrations/`，通过 `sqlx::migrate!` 编译期内嵌；
//!   sqlx 以 `_sqlx_migrations` 记录版本，重复执行是幂等的。
//! - 种子数据见 [`seed`]，可通过 `intimeui-seed` 二进制一键加载。

pub mod seed;

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use crate::models::SeedReport;

/// 编译期内嵌的迁移集合（相对 `server/` crate 根目录）。
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// 数据库错误。
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// `DATABASE_URL` 格式非法。
    #[error("invalid database url: {0}")]
    InvalidUrl(String),
    /// 无法创建数据库文件所在目录。
    #[error("failed to prepare database directory: {0}")]
    Io(#[from] std::io::Error),
    /// SQLx 运行时错误。
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    /// 迁移执行错误。
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

/// 数据库句柄（内部为 SQLite 连接池，可安全克隆）。
#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// 按 `DATABASE_URL` 建立连接池。
    ///
    /// 文件型数据库会自动创建父目录并开启 WAL；`sqlite::memory:` 使用单连接内存库。
    pub async fn connect(url: &str) -> Result<Self, DbError> {
        let options = SqliteConnectOptions::from_str(url)
            .map_err(|error| DbError::InvalidUrl(error.to_string()))?
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        let is_memory = options.get_filename() == Path::new(":memory:");
        if !is_memory {
            if let Some(parent) = options.get_filename().parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
        }

        let options = if is_memory {
            options
        } else {
            options
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal)
        };

        let pool = SqlitePoolOptions::new()
            .max_connections(if is_memory { 1 } else { 5 })
            .connect_with(options)
            .await?;

        Ok(Self { pool })
    }

    /// 建立单连接内存库（测试专用）。
    pub async fn connect_in_memory() -> Result<Self, DbError> {
        Self::connect("sqlite::memory:").await
    }

    /// 执行全部迁移；重复调用不会重复执行已应用的版本。
    pub async fn migrate(&self) -> Result<(), DbError> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    /// 一键加载幂等种子数据。
    pub async fn seed(&self) -> Result<SeedReport, DbError> {
        Ok(seed::load(&self.pool).await?)
    }

    /// 底层连接池（仓储层使用）。
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 就绪探测：执行 `SELECT 1`。
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
}
