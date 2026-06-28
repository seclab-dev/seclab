//! Compose 套件中心模型：清单、目录、实例与应用入口。

use crate::state::DbPool;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;

/// 套件清单顶层结构。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteManifest {
    pub api_version: String,
    pub kind: String,
    pub metadata: SuiteManifestInfo,
    pub runtime: SuiteRuntime,
    #[serde(default)]
    pub app_entries: Vec<SuiteAppEntryManifest>,
    /// 仅覆盖套件中心、应用库和桌面入口的展示文案，不参与运行时配置。
    #[serde(default)]
    pub i18n: Option<SuiteManifestI18n>,
}

/// 套件基础信息。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteManifestInfo {
    pub suite_id: String,
    pub slug: String,
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    pub icon: String,
    #[serde(default)]
    pub category: Option<String>,
}

/// 套件清单国际化覆盖配置。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteManifestI18n {
    pub default_locale: String,
    #[serde(default)]
    pub locales: HashMap<String, SuiteLocaleOverride>,
}

/// 指定语言下的套件展示文案覆盖。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SuiteLocaleOverride {
    #[serde(default)]
    pub metadata: SuiteMetadataLocaleOverride,
    #[serde(default)]
    pub app_entries: HashMap<String, SuiteAppEntryLocaleOverride>,
}

/// 指定语言下的套件基础展示文案覆盖。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SuiteMetadataLocaleOverride {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub summary: String,
}

/// 指定语言下的套件应用入口展示文案覆盖。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SuiteAppEntryLocaleOverride {
    #[serde(default)]
    pub title: String,
}

/// 套件运行时声明。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteRuntime {
    #[serde(rename = "type")]
    pub runtime_type: String,
    pub compose_file: String,
    #[serde(default)]
    pub project_name_template: Option<String>,
}

/// 套件应用入口声明。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteAppEntryManifest {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default = "default_width")]
    pub default_width: i64,
    #[serde(default = "default_height")]
    pub default_height: i64,
    #[serde(default = "default_min_width")]
    pub min_width: i64,
    #[serde(default = "default_min_height")]
    pub min_height: i64,
    #[serde(default)]
    pub window: Option<SuiteAppEntryWindow>,
}

/// 套件应用窗口尺寸声明。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteAppEntryWindow {
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub min_width: Option<i64>,
    pub min_height: Option<i64>,
}

/// 上传包内可转发到 Agent 的二进制安全文件。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuitePackageFile {
    pub path: String,
    pub content_base64: String,
}

/// 已导入套件包的存储形态。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuitePackageSnapshot {
    pub files: Vec<SuitePackageFile>,
}

/// 套件目录项。
#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteCatalogItem {
    pub suite_id: String,
    pub version: String,
    pub name: String,
    pub summary: String,
    pub icon: String,
    pub status: String,
    pub checksum: String,
    pub created_at: String,
    pub updated_at: String,
    pub category: Option<String>,
}

/// 套件实例摘要。
#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteInstanceSummary {
    pub instance_id: String,
    pub suite_id: String,
    pub version: String,
    pub node_id: String,
    pub compose_project_name: String,
    pub status: String,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 套件中心列表响应。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiteListResponse {
    pub catalog: Vec<SuiteCatalogItem>,
    pub instances: Vec<SuiteInstanceSummary>,
}

/// 套件应用入口记录。
#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SuiteAppEntryRecord {
    pub app_id: String,
    pub suite_instance_id: String,
    pub app_entry_id: String,
    pub title: String,
    pub icon: String,
    pub entry_type: String,
    pub entry_target: String,
    pub default_width: i64,
    pub default_height: i64,
    pub min_width: i64,
    pub min_height: i64,
    pub sort_order: i64,
    pub enabled: i64,
}

/// 读取套件中心列表。
pub async fn list_suites(pool: &DbPool, locale: Option<&str>) -> sqlx::Result<SuiteListResponse> {
    let catalog_rows = sqlx::query_as::<_, SuiteCatalogRow>(
        r#"
        SELECT suite_id,
               version,
               manifest_json,
               status,
               checksum,
               created_at,
               updated_at
        FROM suite_catalog_items
        ORDER BY updated_at DESC, suite_id ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    // name/summary 需要根据请求语言从 manifest_json 解析，不能直接使用表中的默认文案。
    let catalog = catalog_rows
        .into_iter()
        .map(|row| row.into_catalog_item(locale))
        .collect::<sqlx::Result<Vec<_>>>()?;

    let instances = sqlx::query_as::<_, SuiteInstanceSummary>(
        r#"
        SELECT instance_id,
               suite_id,
               version,
               node_id,
               compose_project_name,
               status,
               last_error,
               created_at,
               updated_at
        FROM suite_instances
        ORDER BY updated_at DESC, instance_id ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(SuiteListResponse { catalog, instances })
}

#[derive(Debug, FromRow)]
struct SuiteCatalogRow {
    suite_id: String,
    version: String,
    manifest_json: String,
    status: String,
    checksum: String,
    created_at: String,
    updated_at: String,
}

impl SuiteCatalogRow {
    fn into_catalog_item(self, locale: Option<&str>) -> sqlx::Result<SuiteCatalogItem> {
        let manifest = serde_json::from_str::<SuiteManifest>(&self.manifest_json)
            .map_err(|err| sqlx::Error::Decode(Box::new(err)))?;
        // 图标路径仍使用清单原始值，只有用户可见文案参与国际化覆盖。
        let metadata = manifest.localized_metadata(locale);
        Ok(SuiteCatalogItem {
            icon: format!(
                "/api/v1/suites/{}/{}",
                self.suite_id, manifest.metadata.icon
            ),
            suite_id: self.suite_id,
            version: self.version,
            name: metadata.name,
            summary: metadata.summary,
            status: self.status,
            checksum: self.checksum,
            created_at: self.created_at,
            updated_at: self.updated_at,
            category: manifest.metadata.category.clone(),
        })
    }
}

/// 解析后的套件展示文案。
#[derive(Debug, Clone)]
pub struct SuiteLocalizedMetadata {
    pub name: String,
    pub summary: String,
}

impl SuiteManifest {
    /// 按语言选择套件基础展示文案，缺失翻译时回退到默认字段。
    pub fn localized_metadata(&self, locale: Option<&str>) -> SuiteLocalizedMetadata {
        let override_value = self.resolve_locale_override(locale);
        SuiteLocalizedMetadata {
            name: override_value
                .and_then(|value| non_empty_string(&value.metadata.name))
                .unwrap_or_else(|| self.metadata.name.clone()),
            summary: override_value
                .and_then(|value| non_empty_string(&value.metadata.summary))
                .unwrap_or_else(|| self.metadata.summary.clone()),
        }
    }

    /// 按语言选择应用入口标题，缺失翻译时回退到入口默认标题。
    pub fn localized_app_entry_title(
        &self,
        entry: &SuiteAppEntryManifest,
        locale: Option<&str>,
    ) -> String {
        self.resolve_locale_override(locale)
            .and_then(|value| value.app_entries.get(&entry.id))
            .and_then(|value| non_empty_string(&value.title))
            .unwrap_or_else(|| entry.title.clone())
    }

    fn resolve_locale_override(&self, locale: Option<&str>) -> Option<&SuiteLocaleOverride> {
        let i18n = self.i18n.as_ref()?;
        let requested = locale
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(i18n.default_locale.as_str());
        // 依次匹配完整 locale、下划线归一化后的 locale，以及语言主标签。
        i18n.locales
            .get(requested)
            .or_else(|| i18n.locales.get(&requested.replace('_', "-")))
            .or_else(|| {
                requested
                    .split_once('-')
                    .and_then(|(language, _)| i18n.locales.get(language))
            })
    }
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// 写入或更新套件目录项。
pub async fn upsert_catalog_item(
    pool: &DbPool,
    manifest: &SuiteManifest,
    package: &SuitePackageSnapshot,
    checksum: &str,
) -> sqlx::Result<()> {
    let manifest_json = serde_json::to_string(manifest).unwrap_or_else(|_| "{}".to_string());
    let package_json = serde_json::to_string(package).unwrap_or_else(|_| "{}".to_string());
    sqlx::query(
        r#"
        INSERT INTO suite_catalog_items (
            suite_id, version, name, summary, manifest_json, package_json, checksum, status
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'available')
        ON CONFLICT(suite_id) DO UPDATE SET
            version = excluded.version,
            name = excluded.name,
            summary = excluded.summary,
            manifest_json = excluded.manifest_json,
            package_json = excluded.package_json,
            checksum = excluded.checksum,
            status = 'available'
        "#,
    )
    .bind(&manifest.metadata.suite_id)
    .bind(&manifest.metadata.version)
    .bind(&manifest.metadata.name)
    .bind(&manifest.metadata.summary)
    .bind(manifest_json)
    .bind(package_json)
    .bind(checksum)
    .execute(pool)
    .await?;
    Ok(())
}

/// 读取套件 manifest 与包快照。
pub async fn fetch_catalog_payload(
    pool: &DbPool,
    suite_id: &str,
) -> sqlx::Result<Option<(SuiteManifest, SuitePackageSnapshot)>> {
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT manifest_json, package_json FROM suite_catalog_items WHERE suite_id = ?1",
    )
    .bind(suite_id)
    .fetch_optional(pool)
    .await?;

    let Some((manifest_json, package_json)) = row else {
        return Ok(None);
    };
    let manifest = serde_json::from_str::<SuiteManifest>(&manifest_json)
        .map_err(|err| sqlx::Error::Decode(Box::new(err)))?;
    let package = serde_json::from_str::<SuitePackageSnapshot>(&package_json)
        .map_err(|err| sqlx::Error::Decode(Box::new(err)))?;
    Ok(Some((manifest, package)))
}

/// 删除套件目录项。调用方必须先确认该套件没有已安装实例。
pub async fn delete_catalog_item(pool: &DbPool, suite_id: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM suite_catalog_items WHERE suite_id = ?1")
        .bind(suite_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 创建套件实例记录。
pub async fn insert_instance(pool: &DbPool, instance: &SuiteInstanceSummary) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO suite_instances (
            instance_id, suite_id, version, node_id, compose_project_name, status, last_error
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
    )
    .bind(&instance.instance_id)
    .bind(&instance.suite_id)
    .bind(&instance.version)
    .bind(&instance.node_id)
    .bind(&instance.compose_project_name)
    .bind(&instance.status)
    .bind(&instance.last_error)
    .execute(pool)
    .await?;
    Ok(())
}

/// 读取套件实例。
pub async fn fetch_instance(
    pool: &DbPool,
    instance_id: &str,
) -> sqlx::Result<Option<SuiteInstanceSummary>> {
    sqlx::query_as::<_, SuiteInstanceSummary>(
        r#"
        SELECT instance_id,
               suite_id,
               version,
               node_id,
               compose_project_name,
               status,
               last_error,
               created_at,
               updated_at
        FROM suite_instances
        WHERE instance_id = ?1
        "#,
    )
    .bind(instance_id)
    .fetch_optional(pool)
    .await
}

/// 按套件 ID 读取已安装实例。当前套件系统按单例运行，最多返回一个实例。
pub async fn fetch_instance_by_suite_id(
    pool: &DbPool,
    suite_id: &str,
) -> sqlx::Result<Option<SuiteInstanceSummary>> {
    sqlx::query_as::<_, SuiteInstanceSummary>(
        r#"
        SELECT instance_id,
               suite_id,
               version,
               node_id,
               compose_project_name,
               status,
               last_error,
               created_at,
               updated_at
        FROM suite_instances
        WHERE suite_id = ?1
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )
    .bind(suite_id)
    .fetch_optional(pool)
    .await
}

/// 更新实例状态。
pub async fn update_instance_status(
    pool: &DbPool,
    instance_id: &str,
    status: &str,
    last_error: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE suite_instances
           SET status = ?2,
               last_error = ?3
         WHERE instance_id = ?1
        "#,
    )
    .bind(instance_id)
    .bind(status)
    .bind(last_error)
    .execute(pool)
    .await?;
    Ok(())
}

/// 替换实例对应的应用入口。
pub async fn replace_instance_app_entries(
    pool: &DbPool,
    instance_id: &str,
    entries: &[SuiteAppEntryRecord],
) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM suite_app_entries WHERE suite_instance_id = ?1")
        .bind(instance_id)
        .execute(&mut *tx)
        .await?;

    for entry in entries {
        sqlx::query(
            r#"
            INSERT INTO suite_app_entries (
                app_id,
                suite_instance_id,
                app_entry_id,
                title,
                icon,
                entry_type,
                entry_target,
                default_width,
                default_height,
                min_width,
                min_height,
                sort_order,
                enabled
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
        )
        .bind(&entry.app_id)
        .bind(&entry.suite_instance_id)
        .bind(&entry.app_entry_id)
        .bind(&entry.title)
        .bind(&entry.icon)
        .bind(&entry.entry_type)
        .bind(&entry.entry_target)
        .bind(entry.default_width)
        .bind(entry.default_height)
        .bind(entry.min_width)
        .bind(entry.min_height)
        .bind(entry.sort_order)
        .bind(entry.enabled)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// 删除实例应用入口。
pub async fn delete_instance_app_entries(pool: &DbPool, instance_id: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM suite_app_entries WHERE suite_instance_id = ?1")
        .bind(instance_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 删除套件实例。
pub async fn delete_instance(pool: &DbPool, instance_id: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM suite_instances WHERE instance_id = ?1")
        .bind(instance_id)
        .execute(pool)
        .await?;
    Ok(())
}

fn default_width() -> i64 {
    1024
}

fn default_height() -> i64 {
    720
}

fn default_min_width() -> i64 {
    860
}

fn default_min_height() -> i64 {
    560
}
