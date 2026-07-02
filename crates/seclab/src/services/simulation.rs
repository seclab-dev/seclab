//! 协议仿真调度服务（SecSim Controller Service）：管理仿真的动态生命周期调度及与 Agent 的 mTLS 通信。

use crate::models::node_runtime_client::NodeRuntimeClient;
use crate::models::simulation::{
    SimInstanceRecord, delete_sim_instance, get_sim_instance_by_id, get_sim_instance_by_node_port,
    get_sim_rule_by_id, insert_sim_instance,
};
use crate::services::simulation_protocols::{deployable_protocols_label, is_deployable_protocol};
use crate::state::DbPool;
use crate::types::{ApiError, ApiResult, new_uuid_v7};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Agent 仿真启动请求结构体。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentStartSimRequest {
    rule_id: String,
    rule_name: String,
    protocol: String,
    port: u16,
    config_yaml: String,
    seclab_callback_url: String,
    node_id: String,
}

/// Agent 仿真停止请求结构体。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentStopSimRequest {
    port: u16,
}

/// 向指定计算节点部署并运行协议仿真。
pub async fn deploy_simulation_service(
    pool: &DbPool,
    node_id: &str,
    port: u16,
    rule_id: i64,
    callback_url: &str,
) -> ApiResult<SimInstanceRecord> {
    // 1. 验证仿真规则是否存在并处于当前支持的协议范围内
    let Some(rule) = get_sim_rule_by_id(pool, rule_id)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
    else {
        return Err(ApiError::not_found(
            seclab_contracts::api::ErrorCode::NodeNotFound,
            format!("Simulation rule {} not found", rule_id),
        ));
    };

    if !is_deployable_protocol(&rule.protocol) {
        return Err(ApiError::BadRequest(format!(
            "Only {} protocol simulation can be deployed at current phase. Rule protocol is {}",
            deployable_protocols_label(),
            rule.protocol
        )));
    }

    // 2. 防御并发冲突：同一个节点的同一个端口不可重复拉起仿真
    if get_sim_instance_by_node_port(pool, node_id, port as i64)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
        .is_some()
    {
        return Err(ApiError::conflict(
            seclab_contracts::api::ErrorCode::ValidationFailed,
            "app.simulation.deployments.messages.portOccupied",
        ));
    }

    // 3. 智能修正保护：如果部署节点是非本地节点，且传入的回调是 localhost/127.0.0.1 或 5173，自动重写为控制端对外探测的真实 IP 和 7310 主端口
    let final_callback_url = {
        let is_local_target = node_id == "local";
        let is_localhost_callback = callback_url.contains("localhost")
            || callback_url.contains("127.0.0.1")
            || callback_url.contains(":5173");

        if !is_local_target && is_localhost_callback {
            let base_seclab_url = crate::services::node_deploy::resolve_seclab_url();
            let trimmed_base = base_seclab_url.trim_end_matches('/');
            format!("{}/api/v1/simulation-public/log", trimmed_base)
        } else {
            callback_url.to_string()
        }
    };

    // 4. 构建 mTLS 安全客户端，向边缘 Agent 发送拉起指令
    let client = NodeRuntimeClient::from_node_route(pool, Some(node_id)).await?;
    let agent_payload = AgentStartSimRequest {
        rule_id: rule_id.to_string(),
        rule_name: rule.name.clone(),
        protocol: rule.protocol.clone(),
        port,
        config_yaml: rule.config_yaml.clone(),
        seclab_callback_url: final_callback_url,
        node_id: node_id.to_string(),
    };

    // 调用 Agent API 接口
    let agent_resp: serde_json::Value = client
        .post_json("/api/v1/agent/simulation/start", &agent_payload)
        .await
        .map_err(|err| {
            ApiError::bad_request(
                seclab_contracts::api::ErrorCode::AgentRequestFailed,
                format!("Failed to dispatch simulation command to agent: {}", err),
            )
        })?;

    // 检查 Agent 反馈状态
    let success = agent_resp
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let status_success = success
        || agent_resp
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| s == "success")
            .unwrap_or(false);

    if !status_success {
        let err_detail = {
            let msg_str = agent_resp
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            let data_str = agent_resp
                .get("data")
                .and_then(|d| d.as_str())
                .unwrap_or("");

            if !msg_str.is_empty() && msg_str != "internal server error" {
                msg_str.to_string()
            } else if !data_str.is_empty() {
                data_str.to_string()
            } else if !msg_str.is_empty() {
                msg_str.to_string()
            } else {
                format!("{:?}", agent_resp)
            }
        };
        let error_code = if err_detail.contains("bind")
            || err_detail.contains("port")
            || err_detail.contains("address already in use")
        {
            seclab_contracts::api::ErrorCode::SimulationPortUnavailable
        } else {
            seclab_contracts::api::ErrorCode::BadRequest
        };
        return Err(ApiError::bad_request(
            error_code,
            format!("Agent deployment failed: {}", err_detail),
        ));
    }

    // 4. 下发成功，将运行实例落入控制端本地数据库进行状态持久化
    let now = Utc::now().to_rfc3339();
    let instance = SimInstanceRecord {
        instance_id: new_uuid_v7(),
        node_id: node_id.to_string(),
        rule_id,
        listen_port: port as i64,
        status: "active".to_string(),
        error_message: None,
        pcap_status: "idle".to_string(),
        pcap_start_time: None,
        pcap_file_path: None,
        created_at: now.clone(),
        updated_at: now,
    };

    insert_sim_instance(pool, &instance)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(instance)
}

/// 注销并停止特定计算节点上的协议仿真服务。
pub async fn undeploy_simulation_service(pool: &DbPool, instance_id: &str) -> ApiResult<()> {
    // 1. 获取活动实例
    let Some(instance) = get_sim_instance_by_id(pool, instance_id)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
    else {
        return Err(ApiError::not_found(
            seclab_contracts::api::ErrorCode::NodeNotFound,
            format!("Simulation instance {} not found", instance_id),
        ));
    };

    // 2. 构建 mTLS 安全客户端，向 Agent 发送解绑与关闭指令
    let client = NodeRuntimeClient::from_node_route(pool, Some(&instance.node_id)).await?;
    let agent_payload = AgentStopSimRequest {
        port: instance.listen_port as u16,
    };

    // 允许 Agent 的清理返回 404（即可能进程已被关闭但端口尚未释放等宽泛场景）以增强容错性
    if let Err(err) = client
        .post_json::<serde_json::Value, _>("/api/v1/agent/simulation/stop", &agent_payload)
        .await
    {
        tracing::warn!(
            "Undeploy simulation sent to agent failed (ignoring to proceed local cleanup): {:?}",
            err
        );
    }

    // 3. 从控制端本地数据库删除实例记录，完成清理
    delete_sim_instance(pool, instance_id)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::simulation::{
        BUILTIN_SIM_RULE_MAX_ID, SimInstanceRecord, SimLogRecord, SimRuleRecord,
        get_sim_instance_by_id, get_sim_rule_by_id, insert_custom_sim_rule_deduplicated,
        insert_sim_instance, insert_sim_log, insert_sim_rule, list_sim_logs_by_node,
        list_sim_rules,
    };
    use crate::test_support::setup_test_db;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_simulation_rule_crud() {
        let pool = setup_test_db().await;
        let rule_id: i64 = 10001;
        let now = Utc::now().to_rfc3339();

        let record = SimRuleRecord {
            id: rule_id,
            name: "Test HTTP Rule".to_string(),
            name_en: "Test HTTP Rule".to_string(),
            cve: None,
            category: "vuln_sim".to_string(),
            description_zh: "Test HTTP Rule".to_string(),
            description_en: "Test HTTP Rule".to_string(),
            protocol: "http".to_string(),
            default_port: Some(8080),
            config_yaml: "{}".to_string(),
            source_type: "custom".to_string(),
            source_package_id: None,
            rule_status: "active".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        insert_sim_rule(&pool, &record).await.unwrap();

        let fetched = get_sim_rule_by_id(&pool, rule_id).await.unwrap().unwrap();
        assert_eq!(fetched.name, "Test HTTP Rule");
        assert_eq!(fetched.protocol, "http");

        let list = list_sim_rules(&pool).await.unwrap();
        assert!(!list.is_empty());
    }

    #[tokio::test]
    async fn custom_rule_creation_uses_incremental_ids_and_deduplicates() {
        let pool = setup_test_db().await;
        let now = Utc::now().to_rfc3339();

        let legacy_timestamp_rule = SimRuleRecord {
            id: 1_782_972_862_272_802,
            name: "Legacy Timestamp Rule".to_string(),
            name_en: "Legacy Timestamp Rule".to_string(),
            cve: None,
            category: "custom".to_string(),
            description_zh: "Legacy Timestamp Rule".to_string(),
            description_en: "".to_string(),
            protocol: "rdp".to_string(),
            default_port: Some(3389),
            config_yaml: r#"{"flags":9}"#.to_string(),
            source_type: "custom".to_string(),
            source_package_id: None,
            rule_status: "active".to_string(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        insert_sim_rule(&pool, &legacy_timestamp_rule)
            .await
            .unwrap();

        let first = SimRuleRecord {
            id: 0,
            name: "RDP Rule".to_string(),
            name_en: "RDP Rule".to_string(),
            cve: None,
            category: "custom".to_string(),
            description_zh: "RDP Rule".to_string(),
            description_en: "".to_string(),
            protocol: "rdp".to_string(),
            default_port: Some(3389),
            config_yaml: r#"{"flags":1}"#.to_string(),
            source_type: "custom".to_string(),
            source_package_id: None,
            rule_status: "active".to_string(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        let inserted = insert_custom_sim_rule_deduplicated(&pool, first.clone(), None)
            .await
            .unwrap();
        assert_eq!(inserted.id, BUILTIN_SIM_RULE_MAX_ID + 1);

        let duplicate = insert_custom_sim_rule_deduplicated(&pool, first, None)
            .await
            .unwrap();
        assert_eq!(duplicate.id, inserted.id);

        let second = SimRuleRecord {
            id: 0,
            name: "RDP Rule 2".to_string(),
            config_yaml: r#"{"flags":2}"#.to_string(),
            ..inserted.clone()
        };
        let inserted_second = insert_custom_sim_rule_deduplicated(&pool, second, None)
            .await
            .unwrap();
        assert_eq!(inserted_second.id, BUILTIN_SIM_RULE_MAX_ID + 2);
    }

    #[tokio::test]
    async fn test_simulation_instance_and_log_crud() {
        let pool = setup_test_db().await;
        let instance_id = Uuid::new_v4().to_string();
        let node_id = "node-test-1".to_string();
        let rule_id: i64 = 10002;
        let now = Utc::now().to_rfc3339();

        // 1. 插入关联 of rule
        let rule = SimRuleRecord {
            id: rule_id,
            name: "Test Rule".to_string(),
            name_en: "Test Rule".to_string(),
            cve: None,
            category: "vuln_sim".to_string(),
            description_zh: "Test Rule".to_string(),
            description_en: "Test Rule".to_string(),
            protocol: "http".to_string(),
            default_port: Some(8080),
            config_yaml: "{}".to_string(),
            source_type: "custom".to_string(),
            source_package_id: None,
            rule_status: "active".to_string(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        insert_sim_rule(&pool, &rule).await.unwrap();

        // 2. 插入关联的 node
        crate::models::nodes::insert_node(
            &pool,
            &crate::models::nodes::NewNodeRecord {
                node_id: node_id.clone(),
                tenant_id: None,
                name: "node-test-1".to_string(),
                normalized_name: "node-test-1".to_string(),
                group_name: "default".to_string(),
                labels: "[]".to_string(),
                description: None,
                desired_role: None,
                schedulable: true,
                metadata: "{}".to_string(),
            },
        )
        .await
        .unwrap();

        // 3. 插入运行实例
        let instance = SimInstanceRecord {
            instance_id: instance_id.clone(),
            node_id: node_id.clone(),
            rule_id,
            listen_port: 8080,
            status: "active".to_string(),
            error_message: None,
            pcap_status: "idle".to_string(),
            pcap_start_time: None,
            pcap_file_path: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        insert_sim_instance(&pool, &instance).await.unwrap();

        let fetched = get_sim_instance_by_id(&pool, &instance_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.node_id, "node-test-1");
        assert_eq!(fetched.listen_port, 8080);

        let log = SimLogRecord {
            log_id: None,
            instance_id: instance_id.clone(),
            node_id: node_id.clone(),
            client_ip: "192.168.1.100".to_string(),
            client_port: 43210,
            event_type: "exploit_attempt".to_string(),
            detail_summary: "Simulated exploit attempt blocked".to_string(),
            payload_hex: Some("01020304".to_string()),
            pcap_file_path: None,
            timestamp: now,
        };

        insert_sim_log(&pool, &log).await.unwrap();

        let logs = list_sim_logs_by_node(&pool, &node_id, 10).await.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].client_ip, "192.168.1.100");
        assert_eq!(logs[0].event_type, "exploit_attempt");
        assert_eq!(logs[0].pcap_file_path, None);

        // 4. 测试 PCAP 关联更新
        crate::models::simulation::update_sim_log_pcap(
            &pool,
            &node_id,
            43210,
            "192.168.1.100",
            "/data/pcaps/test.pcap",
        )
        .await
        .unwrap();

        let logs_updated = list_sim_logs_by_node(&pool, &node_id, 10).await.unwrap();
        assert_eq!(logs_updated.len(), 1);
        assert_eq!(
            logs_updated[0].pcap_file_path,
            Some("/data/pcaps/test.pcap".to_string())
        );
    }
}
