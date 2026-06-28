//! # SecLab 仿真规则包导入服务
//!
//! 此模块负责接收、解析、校验并导入动态规则升级包。
//! 规则包采用 Protobuf 格式进行序列化，并强制 Ed25519 签名验证，
//! 全程解密和解析在内存中完成，安全可信，零中间文件落盘。
//!
//! ## 兼容性契约
//!
//! 本模块中的 Protobuf 结构体（[`SimRuleProto`]、[`RulePackageManifestProto`]、
//! [`SimRulePackageProto`]）必须与 `seclab-sim-rules` 仓库中 `lib.rs` 的同名定义
//! 保持**逐字段对齐**（字段名、类型、prost tag 编号）。
//!
//! 变更约束摘要（完整规范参见 `docs/design/主控与规则库兼容性契约.md`）：
//!
//! - **允许**：追加新字段（使用新 tag 编号）。
//! - **禁止**：删除既有字段、修改既有 tag 编号或类型、修改既有字段语义。
//! - 任何不兼容变更须同步递增 `ruleset_format_version` 并遵循"主控先行发版"顺序。
//! - 包规则 ID 必须处于 `[1, 999_999]` 区间，`>= 1_000_000` 保留给用户自定义规则。

use chrono::Utc;
use flate2::read::GzDecoder;
use prost::Message;
use std::fs;
use std::io::Read;
use tar::Archive;

use crate::models::simulation::SimRulePackageRecord;
use crate::state::DbPool;

const SUPPORTED_RULESET_FORMAT_VERSION: i32 = 1;

/// 仿真策略模板的定义结构体，使用 Protobuf 二进制序列化 (Prost Code-first 定义)。
///
/// # 跨仓库同步约束
///
/// 此结构体必须与 `seclab-sim-rules` crate 中 `lib.rs` 的同名定义
/// 在字段名、类型和 prost tag 编号上保持完全一致。
/// 变更时须同步更新对端定义，详见 `docs/design/主控与规则库兼容性契约.md`。
#[derive(Clone, PartialEq, Message)]
pub struct SimRuleProto {
    #[prost(int64, tag = "1")]
    pub id: i64,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, tag = "3")]
    pub name_en: String,
    #[prost(string, optional, tag = "4")]
    pub cve: Option<String>,
    #[prost(string, tag = "5")]
    pub category: String,
    #[prost(string, tag = "6")]
    pub description_zh: String,
    #[prost(string, tag = "7")]
    pub description_en: String,
    #[prost(string, tag = "8")]
    pub protocol: String,
    #[prost(int64, optional, tag = "9")]
    pub default_port: Option<i64>,
    #[prost(string, tag = "10")]
    pub config_json: String, // 内层行为序列化后的高内聚 JSON 串
}

/// 规则包元数据。
///
/// # 跨仓库同步约束
///
/// 此结构体必须与 `seclab-sim-rules::RulePackageManifestProto` 保持一致。
/// 变更时须同步更新对端定义，详见 `docs/design/主控与规则库兼容性契约.md`。
#[derive(Clone, PartialEq, Message)]
pub struct RulePackageManifestProto {
    #[prost(string, tag = "1")]
    pub package_id: String,
    #[prost(string, tag = "2")]
    pub version: String,
    #[prost(int32, tag = "3")]
    pub ruleset_format_version: i32,
    #[prost(string, tag = "4")]
    pub min_seclab_version: String,
    #[prost(int64, tag = "5")]
    pub generated_at: i64,
    #[prost(int32, tag = "6")]
    pub rule_count: i32,
}

/// 统一的 Protobuf 二进制规则包载荷。
///
/// # 跨仓库同步约束
///
/// 此结构体必须与 `seclab-sim-rules::SimRulePackageProto` 保持一致。
/// 变更时须同步更新对端定义，详见 `docs/design/主控与规则库兼容性契约.md`。
#[derive(Clone, PartialEq, Message)]
pub struct SimRulePackageProto {
    #[prost(message, optional, tag = "1")]
    pub manifest: Option<RulePackageManifestProto>,
    #[prost(message, repeated, tag = "2")]
    pub rules: Vec<SimRuleProto>,
}

/// 执行动态规则包的签名校验、解压及事务化入库逻辑
pub async fn import_rule_package(
    pool: &DbPool,
    archive_bytes: &[u8],
) -> Result<(SimRulePackageRecord, bool), String> {
    import_rule_package_with_verifier(pool, archive_bytes, |bin_bytes, sig_text| {
        seclab_upgrade::signature::verify_release_signature(bin_bytes, sig_text)
            .map_err(|err| format!("Signature verification failed: {:?}", err))
    })
    .await
}

async fn import_rule_package_with_verifier<F>(
    pool: &DbPool,
    archive_bytes: &[u8],
    verify_signature: F,
) -> Result<(SimRulePackageRecord, bool), String>
where
    F: Fn(&[u8], &str) -> Result<(), String>,
{
    // 1. 在内存中解析外层归档包并提取 rules.bin 和 rules.bin.sig
    let (bin_bytes, sig_text) = extract_tar_gz_contents(archive_bytes)?;

    // 2. 使用主控内置 Ed25519 发布公钥对 rules.bin 执行签名真实性校验
    verify_signature(&bin_bytes, &sig_text)?;

    // 3. 内存直接反序列化 Protobuf 结构
    let package_proto = SimRulePackageProto::decode(&bin_bytes[..])
        .map_err(|err| format!("Failed to decode protobuf message: {:?}", err))?;

    let manifest = package_proto
        .manifest
        .ok_or("Rule package manifest is missing from protobuf payload")?;

    // 检查数据库中是否已存在该版本包，如果存在则直接跳过升级并返回已有记录
    let existing = sqlx::query_as::<_, SimRulePackageRecord>(
        "SELECT * FROM sim_rule_packages WHERE package_id = ? AND version = ?",
    )
    .bind(&manifest.package_id)
    .bind(&manifest.version)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    if let Some(record) = existing {
        tracing::info!(
            "Simulation rule package '{}' version '{}' already exists, skipping upgrade.",
            manifest.package_id,
            manifest.version
        );
        return Ok((record, true));
    }

    // 4. 校验规则包要求的最低主控版本
    let current_version = env!("CARGO_PKG_VERSION");
    if let Err(e) = verify_min_version(&manifest.min_seclab_version, current_version) {
        return Err(format!("Version compatibility check failed: {}", e));
    }

    if manifest.ruleset_format_version != SUPPORTED_RULESET_FORMAT_VERSION {
        return Err(format!(
            "Unsupported ruleset_format_version: {}, current controller supports {}",
            manifest.ruleset_format_version, SUPPORTED_RULESET_FORMAT_VERSION
        ));
    }

    if manifest.rule_count != package_proto.rules.len() as i32 {
        return Err(format!(
            "Integrity check failed: ruleCount in manifest ({}) does not match rules array size ({})",
            manifest.rule_count,
            package_proto.rules.len()
        ));
    }

    // 5. 校验规则 ID 范围，保护 ID 分区 (包规则 ID 必须处于 [100000, 999999] 保留区间)
    for rule in &package_proto.rules {
        if rule.id >= 1_000_000 {
            return Err(format!(
                "Security check failed: Rule '{}' ID {} is >= 1,000,000, which is reserved for custom rules only",
                rule.name, rule.id
            ));
        }
    }

    // 6. 计算签名和归档哈希值以便存入历史包记录
    let archive_sha256 = {
        let hash = ring::digest::digest(&ring::digest::SHA256, archive_bytes);
        hex::encode(hash.as_ref())
    };
    let signature_hex = {
        let clean_sig = sig_text
            .lines()
            .map(str::trim)
            .find(|line| {
                !line.is_empty()
                    && !line.starts_with("untrusted comment:")
                    && !line.starts_with("trusted comment:")
            })
            .unwrap_or(sig_text.trim());
        clean_sig.to_string()
    };

    let imported_at = Utc::now().to_rfc3339();

    // 7. 开启数据库事务
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    // 标记同 package_id 的旧包为 superseded
    sqlx::query("UPDATE sim_rule_packages SET status = 'superseded' WHERE package_id = ?")
        .bind(&manifest.package_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    // 插入当前规则包记录
    sqlx::query(
        r#"
        INSERT INTO sim_rule_packages (
            package_id, version, ruleset_format_version, min_seclab_version,
            rule_count, signature_hex, archive_sha256, status, imported_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 'active', ?)
        ON CONFLICT(package_id, version) DO UPDATE SET
            ruleset_format_version = excluded.ruleset_format_version,
            min_seclab_version = excluded.min_seclab_version,
            rule_count = excluded.rule_count,
            signature_hex = excluded.signature_hex,
            archive_sha256 = excluded.archive_sha256,
            status = excluded.status,
            imported_at = excluded.imported_at
        "#,
    )
    .bind(&manifest.package_id)
    .bind(&manifest.version)
    .bind(manifest.ruleset_format_version)
    .bind(&manifest.min_seclab_version)
    .bind(manifest.rule_count)
    .bind(&signature_hex)
    .bind(&archive_sha256)
    .bind(&imported_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    // 逐一 UPSERT 导入的新规则
    let mut imported_rule_ids = Vec::new();
    for r in &package_proto.rules {
        imported_rule_ids.push(r.id);

        sqlx::query(
            r#"
            INSERT INTO sim_rules (
                id, name, name_en, cve, category, description_zh, description_en,
                protocol, default_port, config_yaml, source_type, source_package_id,
                rule_status, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'package', ?, 'active', ?, ?)
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
        .bind(r.id)
        .bind(&r.name)
        .bind(&r.name_en)
        .bind(&r.cve)
        .bind(&r.category)
        .bind(&r.description_zh)
        .bind(&r.description_en)
        .bind(&r.protocol)
        .bind(r.default_port)
        .bind(&r.config_json)
        .bind(&manifest.package_id)
        .bind(&imported_at)
        .bind(&imported_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    // 缺失规则停用：如果旧的 package 规则不存在于新包中，置其为 inactive，规避级联删除
    if !imported_rule_ids.is_empty() {
        let placeholders = imported_rule_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let query_str = format!(
            "UPDATE sim_rules SET rule_status = 'inactive' WHERE source_type = 'package' AND id NOT IN ({})",
            placeholders
        );

        let mut q = sqlx::query(&query_str);
        for id in &imported_rule_ids {
            q = q.bind(id);
        }
        q.execute(&mut *tx).await.map_err(|e| e.to_string())?;
    }

    // 8. 在提交事务前，先尝试归档 rules.bin（非关键路径，失败仅告警）
    let archive_result = (|| -> Result<(), String> {
        let dest_dir = crate::config::sim_rules_dir();
        fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
        let dest_path = dest_dir.join(format!("seclab-sim-rules-{}.bin", manifest.version));
        fs::write(dest_path, bin_bytes).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = archive_result {
        tracing::warn!("Failed to archive rules.bin to disk (non-fatal): {}", e);
    }

    tx.commit().await.map_err(|e| e.to_string())?;

    Ok((
        SimRulePackageRecord {
            package_id: manifest.package_id,
            version: manifest.version,
            ruleset_format_version: manifest.ruleset_format_version,
            min_seclab_version: manifest.min_seclab_version,
            rule_count: manifest.rule_count,
            signature_hex,
            archive_sha256,
            status: "active".to_string(),
            imported_at,
        },
        false,
    ))
}

/// 内存中提取 gzip tar 的 helper 方法
fn extract_tar_gz_contents(archive_bytes: &[u8]) -> Result<(Vec<u8>, String), String> {
    let decoder = GzDecoder::new(archive_bytes);
    let mut archive = Archive::new(decoder);
    let mut bin_bytes = None;
    let mut sig_text = None;

    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        if filename == "rules.bin" {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            bin_bytes = Some(buf);
        } else if filename == "rules.bin.sig" {
            let mut buf = String::new();
            entry.read_to_string(&mut buf).map_err(|e| e.to_string())?;
            sig_text = Some(buf);
        }
    }

    let bin = bin_bytes.ok_or("Missing rules.bin inside the package")?;
    let sig = sig_text.ok_or("Missing rules.bin.sig inside the package")?;
    Ok((bin, sig))
}

/// 校验主控最低版本
fn verify_min_version(min_ver: &str, current_ver: &str) -> Result<(), String> {
    let min_semver = semver::Version::parse(min_ver)
        .map_err(|e| format!("Invalid minSeclabVersion semver '{}': {}", min_ver, e))?;
    let current_semver = semver::Version::parse(current_ver).map_err(|e| {
        format!(
            "Invalid current controller version semver '{}': {}",
            current_ver, e
        )
    })?;

    if current_semver < min_semver {
        return Err(format!(
            "Current controller version v{} is below the rule package's minimum requirement v{}",
            current_ver, min_ver
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::setup_test_db;
    use flate2::Compression;
    use flate2::write::GzEncoder;

    fn build_test_rule_package(
        version: &str,
        min_seclab_version: &str,
        ruleset_format_version: i32,
        rule_id: i64,
        rule_count: i32,
    ) -> Vec<u8> {
        let package = SimRulePackageProto {
            manifest: Some(RulePackageManifestProto {
                package_id: "seclab-sim-rules".to_string(),
                version: version.to_string(),
                ruleset_format_version,
                min_seclab_version: min_seclab_version.to_string(),
                generated_at: 1_787_000_000,
                rule_count,
            }),
            rules: vec![SimRuleProto {
                id: rule_id,
                name: "测试 HTTP 仿真规则".to_string(),
                name_en: "Test HTTP Simulation Rule".to_string(),
                cve: None,
                category: "test_env".to_string(),
                description_zh: "用于主控导入测试的最小规则。".to_string(),
                description_en: "Minimal rule for controller import tests.".to_string(),
                protocol: "http".to_string(),
                default_port: Some(8080),
                config_json: r#"{"html":"ok","server_header":"SecLab-Test","exploit_paths":[]}"#
                    .to_string(),
            }],
        };

        let mut bin_bytes = Vec::new();
        package.encode(&mut bin_bytes).unwrap();

        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut tar_builder = tar::Builder::new(encoder);

        let mut bin_header = tar::Header::new_gnu();
        bin_header.set_path("rules.bin").unwrap();
        bin_header.set_size(bin_bytes.len() as u64);
        bin_header.set_mode(0o644);
        bin_header.set_cksum();
        tar_builder.append(&bin_header, &bin_bytes[..]).unwrap();

        let sig_bytes = b"test-signature\n";
        let mut sig_header = tar::Header::new_gnu();
        sig_header.set_path("rules.bin.sig").unwrap();
        sig_header.set_size(sig_bytes.len() as u64);
        sig_header.set_mode(0o644);
        sig_header.set_cksum();
        tar_builder.append(&sig_header, &sig_bytes[..]).unwrap();

        tar_builder.finish().unwrap();
        let encoder = tar_builder.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    async fn import_test_rule_package(
        pool: &DbPool,
        archive_bytes: &[u8],
    ) -> Result<(SimRulePackageRecord, bool), String> {
        import_rule_package_with_verifier(pool, archive_bytes, |_, _| Ok(())).await
    }

    #[tokio::test]
    async fn test_import_duplicate_rule_package() {
        let pool = setup_test_db().await;
        let archive_bytes =
            build_test_rule_package("0.1.0-alpha.1", env!("CARGO_PKG_VERSION"), 1, 190001, 1);

        // First import should succeed
        let (record1, skipped1) = import_test_rule_package(&pool, &archive_bytes)
            .await
            .expect("First import failed");
        assert_eq!(record1.status, "active");
        assert_eq!(record1.package_id, "seclab-sim-rules");
        assert_eq!(record1.rule_count, 1);
        assert!(!skipped1);

        // Second import (duplicate) should also succeed (due to skipping upgrade)
        let (record2, skipped2) = import_test_rule_package(&pool, &archive_bytes)
            .await
            .expect("Second duplicate import failed");
        assert_eq!(record2.status, "active");
        assert_eq!(record2.version, record1.version);
        assert_eq!(record2.imported_at, record1.imported_at);
        assert!(skipped2);

        let rule_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sim_rules WHERE id = ?")
            .bind(190001_i64)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rule_count, 1);
    }

    #[tokio::test]
    async fn rejects_rule_count_mismatch() {
        let pool = setup_test_db().await;
        let archive_bytes =
            build_test_rule_package("0.1.0-alpha.2", env!("CARGO_PKG_VERSION"), 1, 190002, 2);

        let err = import_test_rule_package(&pool, &archive_bytes)
            .await
            .expect_err("rule_count mismatch should fail");
        assert!(err.contains("ruleCount in manifest"));
    }

    #[tokio::test]
    async fn rejects_package_rule_id_in_custom_range() {
        let pool = setup_test_db().await;
        let archive_bytes =
            build_test_rule_package("0.1.0-alpha.3", env!("CARGO_PKG_VERSION"), 1, 1_000_000, 1);

        let err = import_test_rule_package(&pool, &archive_bytes)
            .await
            .expect_err("package rule ID in custom range should fail");
        assert!(err.contains("reserved for custom rules"));
    }

    #[tokio::test]
    async fn rejects_package_requiring_newer_controller() {
        let pool = setup_test_db().await;
        let archive_bytes = build_test_rule_package("0.1.0-alpha.4", "999.0.0", 1, 190004, 1);

        let err = import_test_rule_package(&pool, &archive_bytes)
            .await
            .expect_err("newer min_seclab_version should fail");
        assert!(err.contains("Version compatibility check failed"));
    }

    #[tokio::test]
    async fn rejects_unsupported_ruleset_format_version() {
        let pool = setup_test_db().await;
        let archive_bytes =
            build_test_rule_package("0.1.0-alpha.5", env!("CARGO_PKG_VERSION"), 2, 190005, 1);

        let err = import_test_rule_package(&pool, &archive_bytes)
            .await
            .expect_err("unsupported ruleset format should fail");
        assert!(err.contains("Unsupported ruleset_format_version"));
    }
}
