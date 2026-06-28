//! 节点部署服务：维护 `node_provisioning` 表。

use crate::crypto::encrypt_optional;
use crate::models::node_api_types::{NodeCreatePayload, NodeUpdatePayload};
use crate::models::node_provisioning::{
    NodeProvisioningRecord, get_node_provisioning_by_node_id, upsert_node_provisioning,
};
use crate::state::DbPool;
use chrono::Utc;

fn provisioning_id(node_id: &str) -> String {
    format!("provisioning-{node_id}")
}

fn map_crypto_err(err: crate::crypto::CryptoError) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(err))
}

fn parse_port(value: Option<&str>) -> Option<i64> {
    value.and_then(|item| item.parse::<i64>().ok())
}

/// 部署完成后需要写入 `node_provisioning` 的摘要信息。
#[derive(Debug, Clone)]
pub struct MarkDeployedInput {
    pub node_id: String,
    pub deploy_method: String,
    pub result_status: String,
    pub ssh_addr: String,
    pub ssh_port: String,
    pub ssh_user: String,
    pub auth_mode: String,
    pub install_dir: String,
    pub service_port: String,
    pub seclab_url: String,
}

/// 在部署成功后，将节点部署态写入新表。
pub async fn mark_deployed(pool: &DbPool, input: &MarkDeployedInput) -> sqlx::Result<()> {
    let now = Utc::now().to_rfc3339();
    let existing = get_node_provisioning_by_node_id(pool, &input.node_id).await?;
    let (pwd, key, passphrase) = match existing {
        Some(rec) => (
            rec.ssh_password_ciphertext,
            rec.ssh_private_key_ciphertext,
            rec.ssh_private_key_passphrase_ciphertext,
        ),
        None => (None, None, None),
    };

    let record = NodeProvisioningRecord {
        provisioning_id: provisioning_id(&input.node_id),
        node_id: input.node_id.clone(),
        deploy_method: input.deploy_method.clone(),
        ssh_addr: Some(input.ssh_addr.clone()),
        ssh_port: input.ssh_port.parse::<i64>().ok(),
        ssh_user: Some(input.ssh_user.clone()),
        ssh_auth_mode: Some(input.auth_mode.clone()),
        ssh_password_ciphertext: pwd,
        ssh_private_key_ciphertext: key,
        ssh_private_key_passphrase_ciphertext: passphrase,
        install_dir: input.install_dir.clone(),
        systemd_service_name: "seclab-agent.service".to_string(),
        expected_listen_port: input.service_port.parse::<i64>().ok(),
        seclab_url: Some(input.seclab_url.trim_end_matches('/').to_string()),
        last_deploy_task_id: None,
        last_deploy_result_status: Some(input.result_status.clone()),
        last_deploy_error_summary: None,
        last_deploy_at: Some(now.clone()),
        created_at: now.clone(),
        updated_at: now,
    };
    upsert_node_provisioning(pool, &record).await
}

/// 记录部署/运维操作结果，供升级、修复、卸载等流程复用。
pub async fn record_operation_result(
    pool: &DbPool,
    node_id: &str,
    deploy_method: &str,
    result_status: &str,
    error_summary: Option<String>,
) -> sqlx::Result<()> {
    let now = Utc::now().to_rfc3339();
    let existing = get_node_provisioning_by_node_id(pool, node_id).await?;
    let mut record = existing.unwrap_or_else(|| NodeProvisioningRecord {
        provisioning_id: provisioning_id(node_id),
        node_id: node_id.to_string(),
        deploy_method: deploy_method.to_string(),
        ssh_addr: None,
        ssh_port: None,
        ssh_user: None,
        ssh_auth_mode: None,
        ssh_password_ciphertext: None,
        ssh_private_key_ciphertext: None,
        ssh_private_key_passphrase_ciphertext: None,
        install_dir: "/opt/seclab".to_string(),
        systemd_service_name: "seclab-agent.service".to_string(),
        expected_listen_port: None,
        seclab_url: None,
        last_deploy_task_id: None,
        last_deploy_result_status: None,
        last_deploy_error_summary: None,
        last_deploy_at: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    });
    record.deploy_method = deploy_method.to_string();
    record.last_deploy_result_status = Some(result_status.to_string());
    record.last_deploy_error_summary = error_summary;
    record.last_deploy_at = Some(now);
    upsert_node_provisioning(pool, &record).await
}

/// 根据节点创建载荷直接写入部署记录。
pub async fn create_provisioning_from_payload(
    pool: &DbPool,
    node_id: &str,
    payload: &NodeCreatePayload,
) -> sqlx::Result<()> {
    let now = Utc::now().to_rfc3339();
    let record = NodeProvisioningRecord {
        provisioning_id: provisioning_id(node_id),
        node_id: node_id.to_string(),
        deploy_method: "inventory_create".to_string(),
        ssh_addr: payload.addr.clone(),
        ssh_port: parse_port(payload.port.as_deref()),
        ssh_user: payload.user.clone(),
        ssh_auth_mode: payload.auth_mode.clone(),
        ssh_password_ciphertext: encrypt_optional(payload.pwd.clone()).map_err(map_crypto_err)?,
        ssh_private_key_ciphertext: encrypt_optional(payload.private_key.clone())
            .map_err(map_crypto_err)?,
        ssh_private_key_passphrase_ciphertext: encrypt_optional(
            payload.private_key_passphrase.clone(),
        )
        .map_err(map_crypto_err)?,
        install_dir: {
            let parent = payload
                .install_dir
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or("/opt");
            let parent_trimmed = parent.trim_end_matches('/');
            if parent_trimmed.is_empty() {
                "/seclab".to_string()
            } else {
                format!("{}/seclab", parent_trimmed)
            }
        },
        systemd_service_name: "seclab-agent.service".to_string(),
        expected_listen_port: parse_port(payload.service_port.as_deref()),
        seclab_url: None,
        last_deploy_task_id: None,
        last_deploy_result_status: None,
        last_deploy_error_summary: None,
        last_deploy_at: None,
        created_at: now.clone(),
        updated_at: now,
    };
    upsert_node_provisioning(pool, &record).await
}

/// 根据节点更新载荷直接更新部署记录。
pub async fn update_provisioning_from_payload(
    pool: &DbPool,
    node_id: &str,
    payload: &NodeUpdatePayload,
) -> sqlx::Result<()> {
    let existing = get_node_provisioning_by_node_id(pool, node_id).await?;
    let existing = existing.unwrap_or_else(|| NodeProvisioningRecord {
        provisioning_id: provisioning_id(node_id),
        node_id: node_id.to_string(),
        deploy_method: "inventory_update".to_string(),
        ssh_addr: None,
        ssh_port: None,
        ssh_user: None,
        ssh_auth_mode: None,
        ssh_password_ciphertext: None,
        ssh_private_key_ciphertext: None,
        ssh_private_key_passphrase_ciphertext: None,
        install_dir: crate::config::DEFAULT_PRODUCTION_HOME.to_string(),
        systemd_service_name: "seclab-agent.service".to_string(),
        expected_listen_port: None,
        seclab_url: None,
        last_deploy_task_id: None,
        last_deploy_result_status: None,
        last_deploy_error_summary: None,
        last_deploy_at: None,
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    });

    let record = NodeProvisioningRecord {
        provisioning_id: existing.provisioning_id,
        node_id: existing.node_id,
        deploy_method: "inventory_update".to_string(),
        ssh_addr: payload.addr.clone().or(existing.ssh_addr),
        ssh_port: parse_port(payload.port.as_deref()).or(existing.ssh_port),
        ssh_user: payload.user.clone().or(existing.ssh_user),
        ssh_auth_mode: payload.auth_mode.clone().or(existing.ssh_auth_mode),
        ssh_password_ciphertext: if payload.pwd.is_some() {
            encrypt_optional(payload.pwd.clone()).map_err(map_crypto_err)?
        } else {
            existing.ssh_password_ciphertext
        },
        ssh_private_key_ciphertext: if payload.private_key.is_some() {
            encrypt_optional(payload.private_key.clone()).map_err(map_crypto_err)?
        } else {
            existing.ssh_private_key_ciphertext
        },
        ssh_private_key_passphrase_ciphertext: if payload.private_key_passphrase.is_some() {
            encrypt_optional(payload.private_key_passphrase.clone()).map_err(map_crypto_err)?
        } else {
            existing.ssh_private_key_passphrase_ciphertext
        },
        install_dir: payload.install_dir.clone().unwrap_or(existing.install_dir),
        systemd_service_name: existing.systemd_service_name,
        expected_listen_port: parse_port(payload.service_port.as_deref())
            .or(existing.expected_listen_port),
        seclab_url: payload
            .seclab_url
            .as_ref()
            .map(|value| value.trim_end_matches('/').to_string())
            .or(existing.seclab_url),
        last_deploy_task_id: existing.last_deploy_task_id,
        last_deploy_result_status: existing.last_deploy_result_status,
        last_deploy_error_summary: existing.last_deploy_error_summary,
        last_deploy_at: existing.last_deploy_at,
        created_at: existing.created_at,
        updated_at: Utc::now().to_rfc3339(),
    };

    upsert_node_provisioning(pool, &record).await
}

/// 同步节点本地配置中的主控回连地址。
pub async fn update_node_seclab_url(
    pool: &DbPool,
    node_id: &str,
    seclab_url: &str,
) -> sqlx::Result<()> {
    let trimmed = seclab_url.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Ok(());
    }
    sqlx::query(
        r#"
        UPDATE node_provisioning
        SET seclab_url = ?
        WHERE node_id = ?
        "#,
    )
    .bind(trimmed)
    .bind(node_id)
    .execute(pool)
    .await?;
    Ok(())
}
