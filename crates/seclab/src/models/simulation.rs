//! 协议仿真模型：`sim_rules`、`sim_instances` 和 `sim_logs` 表的定义与基础 CRUD。

use crate::state::DbPool;
use serde::{Deserialize, Serialize};
use sqlx::{Error, FromRow};

/// 内置仿真规则保留 ID 的最大值。
pub const BUILTIN_SIM_RULE_MAX_ID: i64 = 999_999;

/// 自定义仿真规则自动分配 ID 的起始值。
pub const CUSTOM_SIM_RULE_ID_BASE: i64 = 1_000_000;

/// 仿真配置规则记录。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SimRuleRecord {
    pub id: i64,
    pub name: String,
    pub name_en: String,
    pub cve: Option<String>,
    pub category: String,
    pub description_zh: String,
    pub description_en: String,
    pub protocol: String,
    pub default_port: Option<i64>,
    pub config_yaml: String,
    pub source_type: String,
    pub source_package_id: Option<String>,
    pub rule_status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 节点仿真部署实例记录。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SimInstanceRecord {
    pub instance_id: String,
    pub node_id: String,
    pub rule_id: i64,
    pub listen_port: i64,
    pub status: String,
    pub error_message: Option<String>,
    pub pcap_status: String,
    pub pcap_start_time: Option<i64>,
    pub pcap_file_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 仿真交互审计日志记录。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SimLogRecord {
    pub log_id: Option<i64>,
    pub instance_id: String,
    pub node_id: String,
    pub client_ip: String,
    pub client_port: i64,
    pub event_type: String,
    pub detail_summary: String,
    pub payload_hex: Option<String>,
    pub pcap_file_path: Option<String>,
    pub timestamp: String,
}

// --- SimRule CRUD ---

/// 插入仿真配置规则记录。
pub async fn insert_sim_rule(pool: &DbPool, record: &SimRuleRecord) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO sim_rules (
            id,
            name,
            name_en,
            cve,
            category,
            description_zh,
            description_en,
            protocol,
            default_port,
            config_yaml,
            source_type,
            source_package_id,
            rule_status,
            created_at,
            updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            name_en = excluded.name_en,
            cve = excluded.cve,
            category = excluded.category,
            description_zh = excluded.description_zh,
            description_en = excluded.description_en,
            protocol = excluded.protocol,
            default_port = excluded.default_port,
            config_yaml = excluded.config_yaml,
            source_type = excluded.source_type,
            source_package_id = excluded.source_package_id,
            rule_status = excluded.rule_status,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(record.id)
    .bind(&record.name)
    .bind(&record.name_en)
    .bind(&record.cve)
    .bind(&record.category)
    .bind(&record.description_zh)
    .bind(&record.description_en)
    .bind(&record.protocol)
    .bind(record.default_port)
    .bind(&record.config_yaml)
    .bind(&record.source_type)
    .bind(&record.source_package_id)
    .bind(&record.rule_status)
    .bind(&record.created_at)
    .bind(&record.updated_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// 插入自定义仿真规则，并对等价重复提交做幂等保护。
///
/// 若已存在同名、同协议、同端口、同配置的活跃自定义规则，则直接返回已有记录；
/// 否则在事务中分配递增 ID 并插入新规则。
pub async fn insert_custom_sim_rule_deduplicated(
    pool: &DbPool,
    mut record: SimRuleRecord,
    requested_id: Option<i64>,
) -> sqlx::Result<SimRuleRecord> {
    for attempt in 0..5 {
        let mut tx = pool.begin().await?;

        if let Some(existing) = sqlx::query_as::<_, SimRuleRecord>(
            r#"
            SELECT *
              FROM sim_rules
             WHERE source_type = 'custom'
               AND rule_status = 'active'
               AND name = ?
               AND protocol = ?
               AND default_port IS ?
               AND config_yaml = ?
             ORDER BY created_at DESC
             LIMIT 1
            "#,
        )
        .bind(&record.name)
        .bind(&record.protocol)
        .bind(record.default_port)
        .bind(&record.config_yaml)
        .fetch_optional(&mut *tx)
        .await?
        {
            tx.commit().await?;
            return Ok(existing);
        }

        record.id = match requested_id {
            Some(id) => id,
            None => {
                let used_ids = sqlx::query_scalar::<_, i64>(
                    "SELECT id FROM sim_rules WHERE id >= ? ORDER BY id ASC",
                )
                .bind(CUSTOM_SIM_RULE_ID_BASE)
                .fetch_all(&mut *tx)
                .await?;
                let mut next_id = CUSTOM_SIM_RULE_ID_BASE;
                for used_id in used_ids {
                    if used_id == next_id {
                        next_id += 1;
                    } else if used_id > next_id {
                        break;
                    }
                }
                next_id
            }
        };

        let insert_result = sqlx::query(
            r#"
            INSERT INTO sim_rules (
                id,
                name,
                name_en,
                cve,
                category,
                description_zh,
                description_en,
                protocol,
                default_port,
                config_yaml,
                source_type,
                source_package_id,
                rule_status,
                created_at,
                updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(record.id)
        .bind(&record.name)
        .bind(&record.name_en)
        .bind(&record.cve)
        .bind(&record.category)
        .bind(&record.description_zh)
        .bind(&record.description_en)
        .bind(&record.protocol)
        .bind(record.default_port)
        .bind(&record.config_yaml)
        .bind(&record.source_type)
        .bind(&record.source_package_id)
        .bind(&record.rule_status)
        .bind(&record.created_at)
        .bind(&record.updated_at)
        .execute(&mut *tx)
        .await;

        match insert_result {
            Ok(_) => {
                tx.commit().await?;
                return Ok(record);
            }
            Err(err)
                if requested_id.is_none()
                    && attempt < 4
                    && err
                        .as_database_error()
                        .is_some_and(|db_err| db_err.is_unique_violation()) =>
            {
                tx.rollback().await?;
            }
            Err(err) => return Err(err),
        }
    }

    Err(Error::Protocol(
        "failed to allocate custom simulation rule id".to_string(),
    ))
}

/// 根据 ID 获取仿真配置规则。
pub async fn get_sim_rule_by_id(pool: &DbPool, id: i64) -> sqlx::Result<Option<SimRuleRecord>> {
    sqlx::query_as::<_, SimRuleRecord>("SELECT * FROM sim_rules WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// 获取全部有效仿真配置规则。
pub async fn list_sim_rules(pool: &DbPool) -> sqlx::Result<Vec<SimRuleRecord>> {
    sqlx::query_as::<_, SimRuleRecord>(
        "SELECT * FROM sim_rules WHERE rule_status = 'active' ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

/// 删除仿真配置规则。
pub async fn delete_sim_rule(pool: &DbPool, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sim_rules WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
// --- SimInstance CRUD ---

/// 插入或更新仿真运行实例记录。
pub async fn insert_sim_instance(pool: &DbPool, record: &SimInstanceRecord) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO sim_instances (
            instance_id,
            node_id,
            rule_id,
            listen_port,
            status,
            error_message,
            pcap_status,
            pcap_start_time,
            pcap_file_path,
            created_at,
            updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&record.instance_id)
    .bind(&record.node_id)
    .bind(record.rule_id)
    .bind(record.listen_port)
    .bind(&record.status)
    .bind(&record.error_message)
    .bind(&record.pcap_status)
    .bind(record.pcap_start_time)
    .bind(&record.pcap_file_path)
    .bind(&record.created_at)
    .bind(&record.updated_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// 更新仿真实例的抓包状态、启动时间和文件路径。
pub async fn update_sim_instance_pcap(
    pool: &DbPool,
    instance_id: &str,
    pcap_status: &str,
    pcap_start_time: Option<i64>,
    pcap_file_path: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE sim_instances
        SET pcap_status = ?, pcap_start_time = ?, pcap_file_path = ?, updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        WHERE instance_id = ?
        "#,
    )
    .bind(pcap_status)
    .bind(pcap_start_time)
    .bind(pcap_file_path)
    .bind(instance_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 更新仿真实例的状态和错误信息。
pub async fn update_sim_instance_status(
    pool: &DbPool,
    instance_id: &str,
    status: &str,
    error_message: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE sim_instances
        SET status = ?, error_message = ?, updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        WHERE instance_id = ?
        "#,
    )
    .bind(status)
    .bind(error_message)
    .bind(instance_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 根据 ID 获取仿真运行实例。
pub async fn get_sim_instance_by_id(
    pool: &DbPool,
    instance_id: &str,
) -> sqlx::Result<Option<SimInstanceRecord>> {
    sqlx::query_as::<_, SimInstanceRecord>("SELECT * FROM sim_instances WHERE instance_id = ?")
        .bind(instance_id)
        .fetch_optional(pool)
        .await
}

/// 根据节点 ID 和监听端口获取运行实例。
pub async fn get_sim_instance_by_node_port(
    pool: &DbPool,
    node_id: &str,
    port: i64,
) -> sqlx::Result<Option<SimInstanceRecord>> {
    sqlx::query_as::<_, SimInstanceRecord>(
        "SELECT * FROM sim_instances WHERE node_id = ? AND listen_port = ?",
    )
    .bind(node_id)
    .bind(port)
    .fetch_optional(pool)
    .await
}

/// 列出特定节点的全部仿真运行实例。
pub async fn list_sim_instances_by_node(
    pool: &DbPool,
    node_id: &str,
) -> sqlx::Result<Vec<SimInstanceRecord>> {
    sqlx::query_as::<_, SimInstanceRecord>(
        "SELECT * FROM sim_instances WHERE node_id = ? ORDER BY created_at DESC",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
}

/// 删除仿真运行实例。
pub async fn delete_sim_instance(pool: &DbPool, instance_id: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sim_instances WHERE instance_id = ?")
        .bind(instance_id)
        .execute(pool)
        .await?;
    Ok(())
}

// --- SimLog CRUD ---

/// 写入仿真审计日志记录。
pub async fn insert_sim_log(pool: &DbPool, record: &SimLogRecord) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO sim_logs (
            instance_id,
            node_id,
            client_ip,
            client_port,
            event_type,
            detail_summary,
            payload_hex,
            pcap_file_path,
            timestamp
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&record.instance_id)
    .bind(&record.node_id)
    .bind(&record.client_ip)
    .bind(record.client_port)
    .bind(&record.event_type)
    .bind(&record.detail_summary)
    .bind(&record.payload_hex)
    .bind(&record.pcap_file_path)
    .bind(&record.timestamp)
    .execute(pool)
    .await?;

    Ok(())
}

/// 获取特定节点的仿真审计日志列表。
pub async fn list_sim_logs_by_node(
    pool: &DbPool,
    node_id: &str,
    limit: i64,
) -> sqlx::Result<Vec<SimLogRecord>> {
    sqlx::query_as::<_, SimLogRecord>(
        "SELECT * FROM sim_logs WHERE node_id = ? ORDER BY timestamp DESC LIMIT ?",
    )
    .bind(node_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 关联更新仿真审计日志的 PCAP 文件路径（绑定最新的未关联记录）。
pub async fn update_sim_log_pcap(
    pool: &DbPool,
    node_id: &str,
    client_port: i64,
    client_ip: &str,
    pcap_file_path: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE sim_logs
        SET pcap_file_path = ?
        WHERE log_id = (
            SELECT log_id FROM sim_logs
            WHERE node_id = ? AND client_port = ? AND client_ip = ? AND pcap_file_path IS NULL
            ORDER BY timestamp DESC
            LIMIT 1
        )
        "#,
    )
    .bind(pcap_file_path)
    .bind(node_id)
    .bind(client_port)
    .bind(client_ip)
    .execute(pool)
    .await?;
    Ok(())
}

/// 获取全部仿真运行实例。
pub async fn list_all_sim_instances(pool: &DbPool) -> sqlx::Result<Vec<SimInstanceRecord>> {
    sqlx::query_as::<_, SimInstanceRecord>("SELECT * FROM sim_instances ORDER BY created_at DESC")
        .fetch_all(pool)
        .await
}

/// 获取全部仿真审计日志列表。
pub async fn list_all_sim_logs(pool: &DbPool, limit: i64) -> sqlx::Result<Vec<SimLogRecord>> {
    sqlx::query_as::<_, SimLogRecord>("SELECT * FROM sim_logs ORDER BY timestamp DESC LIMIT ?")
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// 查询特定节点的仿真日志总数。
pub async fn count_sim_logs_by_node(pool: &DbPool, node_id: &str) -> sqlx::Result<i64> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sim_logs WHERE node_id = ?")
        .bind(node_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// 查询全部节点的仿真日志总数。
pub async fn count_all_sim_logs(pool: &DbPool) -> sqlx::Result<i64> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sim_logs")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// 分页查询特定节点的仿真审计日志列表。
pub async fn list_sim_logs_by_node_paginated(
    pool: &DbPool,
    node_id: &str,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<SimLogRecord>> {
    sqlx::query_as::<_, SimLogRecord>(
        "SELECT * FROM sim_logs WHERE node_id = ? ORDER BY timestamp DESC LIMIT ? OFFSET ?",
    )
    .bind(node_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// 分页查询全部节点的仿真审计日志列表。
pub async fn list_all_sim_logs_paginated(
    pool: &DbPool,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<SimLogRecord>> {
    sqlx::query_as::<_, SimLogRecord>(
        "SELECT * FROM sim_logs ORDER BY timestamp DESC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

// --- SimRulePackage CRUD ---

/// 仿真规则包导入记录。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SimRulePackageRecord {
    pub package_id: String,
    pub version: String,
    pub ruleset_format_version: i32,
    pub min_seclab_version: String,
    pub rule_count: i32,
    pub signature_hex: String,
    pub archive_sha256: String,
    pub status: String,
    pub imported_at: String,
}

/// 插入新的规则包记录。
pub async fn insert_rule_package(pool: &DbPool, record: &SimRulePackageRecord) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO sim_rule_packages (
            package_id,
            version,
            ruleset_format_version,
            min_seclab_version,
            rule_count,
            signature_hex,
            archive_sha256,
            status,
            imported_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&record.package_id)
    .bind(&record.version)
    .bind(record.ruleset_format_version)
    .bind(&record.min_seclab_version)
    .bind(record.rule_count)
    .bind(&record.signature_hex)
    .bind(&record.archive_sha256)
    .bind(&record.status)
    .bind(&record.imported_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// 将已导入的所有包标记为 superseded
pub async fn supersede_rule_packages(pool: &DbPool, package_id: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE sim_rule_packages SET status = 'superseded' WHERE package_id = ?")
        .bind(package_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 查询当前 active 的规则包。
pub async fn get_active_rule_package(
    pool: &DbPool,
    package_id: &str,
) -> sqlx::Result<Option<SimRulePackageRecord>> {
    sqlx::query_as::<_, SimRulePackageRecord>(
        "SELECT * FROM sim_rule_packages WHERE package_id = ? AND status = 'active'",
    )
    .bind(package_id)
    .fetch_optional(pool)
    .await
}

/// 获取已导入的规则包历史列表。
pub async fn list_rule_packages(pool: &DbPool) -> sqlx::Result<Vec<SimRulePackageRecord>> {
    sqlx::query_as::<_, SimRulePackageRecord>(
        "SELECT * FROM sim_rule_packages ORDER BY imported_at DESC",
    )
    .fetch_all(pool)
    .await
}
