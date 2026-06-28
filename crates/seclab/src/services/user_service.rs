//! 用户服务：用户初始化、密码校验与业务规则封装。

use crate::state::DbPool;
use anyhow::Result;
use bcrypt::{DEFAULT_COST, hash};
use tracing::{debug, error};
// 默认配置
const DEFAULT_ADMIN_USERNAME: &str = "admin";
const DEFAULT_ADMIN_PASSWORD: &str = "admin";

/// 使用 bcrypt 算法计算给定密码的哈希值。
///
/// # 参数
/// - `password`: 需要被哈希的明文密码。
///
/// # 返回
/// - `Result<String, bcrypt::BcryptError>`: 如果成功，返回哈希后的密码字符串；
///   如果失败，返回 bcrypt 库的错误。
///
/// `DEFAULT_COST` 是 bcrypt 的成本因子，代表计算哈希的复杂度。
/// 较高的成本因子可以更好地抵御暴力破解，但也会消耗更多的计算资源。
pub fn hash_password(password: &str) -> Result<String, bcrypt::BcryptError> {
    hash(password, DEFAULT_COST)
}

/// 检查并创建内置的管理员用户（如果尚不存在）。
///
/// 此函数是应用启动过程中的一个关键步骤，应在数据库连接建立和迁移完成后被调用。
///
/// # 功能流程
/// 1. 查询数据库中是否存在用户名为 `DEFAULT_ADMIN_USERNAME` 的用户。
/// 2. 如果用户已存在，则跳过创建，直接返回。
/// 3. 如果用户不存在，则：
///    a. 使用 `hash_password` 函数哈希 `DEFAULT_ADMIN_PASSWORD`。
///    b. 将新的用户名和哈希后的密码插入到 `users` 表中。
///    c. 记录相应的日志信息。
///
/// # 参数
/// - `seclab_db`: 指向 `seclab` 数据库的连接池 (`DbPool`)。
///
/// # 返回
/// - `Result<()>`: 如果操作成功或用户已存在，返回 `Ok(())`；如果哈希或数据库插入失败，
///   返回一个包含错误信息的 `Err`。
pub async fn create_initial_admin_if_not_exists(seclab_db: &DbPool) -> Result<()> {
    let exists = sqlx::query_scalar(r#"SELECT EXISTS(SELECT 1 FROM users WHERE username = ?)"#)
        .bind(DEFAULT_ADMIN_USERNAME)
        .fetch_one(seclab_db)
        .await?;

    if exists {
        debug!(
            "Default admin user '{}' already exists. Skipping creation.",
            DEFAULT_ADMIN_USERNAME
        );
        return Ok(());
    }

    debug!(
        "Default admin user '{}' not found. Creating it with default password: '{}'",
        DEFAULT_ADMIN_USERNAME, DEFAULT_ADMIN_PASSWORD
    );

    let password_hash = match hash_password(DEFAULT_ADMIN_PASSWORD) {
        Ok(hash) => hash,
        Err(e) => {
            error!("Failed to hash default admin password: {}", e);
            return Err(anyhow::anyhow!("Failed to hash admin password"));
        }
    };

    // 插入默认管理员用户
    let result = sqlx::query(
        r#"
        INSERT INTO users (username, password_hash)
        VALUES (?, ?)
        "#,
    )
    .bind(DEFAULT_ADMIN_USERNAME)
    .bind(password_hash)
    .execute(seclab_db)
    .await;

    match result {
        Ok(_) => {
            debug!(
                "Successfully created default admin user: {}/{}",
                DEFAULT_ADMIN_USERNAME, DEFAULT_ADMIN_PASSWORD
            );
            Ok(())
        }
        Err(e) => {
            error!("Failed to insert default admin user into DB: {}", e);
            Err(anyhow::anyhow!("Failed to insert admin user: {}", e))
        }
    }
}
