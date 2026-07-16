//! 配置读取：解析配置文件与默认值，提供全局访问。

use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DEFAULT_PRODUCTION_HOME: &str = "/opt/seclab";
const DEFAULT_DEV_DATA_DIR: &str = ".seclab";
pub const DEFAULT_RUNTIME_PROTOCOL_VERSION: &str = "1";
pub const DEFAULT_AGENT_LISTEN_ADDR: &str = "[::]:7311";

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

/// agent 服务的运行配置（从文件或默认值加载）。
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub mode: Option<String>,
    pub listen_addr: Option<String>,
    pub agent_ip: Option<String>,
    pub agent_id: Option<String>,
    pub seclab_url: Option<String>,
    pub enrollment_token: Option<String>,
    pub compose_root_dir: Option<String>,
    pub stats_sample_interval_secs: Option<u64>,
    pub stats_retention_hours: Option<u64>,
    #[serde(default)]
    pub controller_compatibility: ControllerCompatibilityConfig,
}

/// Agent 对主控版本与 runtime 协议的兼容要求。
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ControllerCompatibilityConfig {
    #[serde(default = "default_runtime_protocol_version")]
    pub runtime_protocol_version: String,
    #[serde(default = "default_min_supported_controller_version")]
    pub min_supported_controller_version: String,
    #[serde(default = "default_true")]
    pub require_semver: bool,
    #[serde(default = "default_false")]
    pub zero_major_requires_exact: bool,
    #[serde(default = "default_false")]
    pub zero_major_requires_prerelease_match: bool,
    #[serde(default = "default_true")]
    pub stable_requires_same_major: bool,
}

impl Default for ControllerCompatibilityConfig {
    fn default() -> Self {
        Self {
            runtime_protocol_version: default_runtime_protocol_version(),
            min_supported_controller_version: default_min_supported_controller_version(),
            require_semver: true,
            zero_major_requires_exact: false,
            zero_major_requires_prerelease_match: false,
            stable_requires_same_major: true,
        }
    }
}

fn default_runtime_protocol_version() -> String {
    DEFAULT_RUNTIME_PROTOCOL_VERSION.to_string()
}

fn default_min_supported_controller_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

static CONFIG: OnceLock<AgentConfig> = OnceLock::new();

/// 读取配置并初始化全局缓存。
pub fn init() {
    let config = load_config().unwrap_or_default();
    let _ = CONFIG.set(config);
}

/// 返回全局配置实例，必要时使用默认值。
pub fn get() -> &'static AgentConfig {
    CONFIG.get_or_init(AgentConfig::default)
}

fn load_config() -> Option<AgentConfig> {
    let raw = fs::read_to_string(config_path()).ok()?;
    toml::from_str(&raw).ok()
}

fn production_home() -> PathBuf {
    env::var_os("SECLAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PRODUCTION_HOME))
}

fn dev_base_dir() -> PathBuf {
    if let Some(value) = env::var_os("SECLAB_DATA_DIR") {
        return PathBuf::from(value);
    }
    if let Some(value) = env::var_os("SECLAB_DEV_HOME") {
        return PathBuf::from(value);
    }
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"));
    let root = find_workspace_root(&cwd).unwrap_or(cwd);
    root.join(DEFAULT_DEV_DATA_DIR)
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if ancestor.join("crates").is_dir() && ancestor.join("frontend").is_dir() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

pub fn config_path() -> PathBuf {
    if let Some(value) = env::var_os("SECLAB_AGENT_CONFIG") {
        return PathBuf::from(value);
    }
    if let Some(value) = env::var_os("SECLAB_CONFIG_DIR") {
        return PathBuf::from(value).join("agent.toml");
    }
    if use_production_layout() {
        production_home().join("config/agent.toml")
    } else {
        dev_base_dir().join("agent.toml")
    }
}

/// 返回数据库数据目录路径。
pub fn data_dir() -> PathBuf {
    let path = if let Some(value) = env::var_os("SECLAB_DB_DIR") {
        PathBuf::from(value)
    } else if use_production_layout() {
        production_home().join("database")
    } else {
        dev_base_dir().join("database")
    };
    to_absolute_path(path)
}

/// 返回 Compose 项目根目录路径。
pub fn compose_root_dir() -> PathBuf {
    let path = if let Some(path) = get().compose_root_dir.as_ref() {
        PathBuf::from(path)
    } else if use_production_layout() {
        production_home().join("data/compose")
    } else {
        dev_base_dir().join("data/compose")
    };
    to_absolute_path(path)
}

fn to_absolute_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        match env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(_) => path,
        }
    }
}

fn use_production_layout() -> bool {
    if env::var_os("SECLAB_DATA_DIR").is_some() {
        return false;
    }
    // 显式设置开发目录时始终按开发布局。
    if env::var_os("SECLAB_DEV_HOME").is_some() {
        return false;
    }
    // 如果存在系统基准目录环境变量，说明被正式纳管部署，即便处于 debug 模式也强制使用生产环境路径布局
    if env::var_os("SECLAB_HOME").is_some() {
        return true;
    }
    !cfg!(debug_assertions)
}

/// 返回统计采样间隔，带最小值保护。
pub fn stats_sample_interval() -> std::time::Duration {
    let seconds = get()
        .stats_sample_interval_secs
        .filter(|value| *value >= 60)
        .unwrap_or(60);
    std::time::Duration::from_secs(seconds)
}

/// 返回统计数据保留时长，带最小值保护。
pub fn stats_retention_hours() -> u64 {
    get()
        .stats_retention_hours
        .filter(|value| *value >= 1)
        .unwrap_or(12)
}

/// 自适应回调地址重写：
/// 解析传入的回调 URL。如果本地配置中指定了专属的 `seclab_url`，
/// 则将传入 URL 的 scheme、host、port 替换为 `seclab_url` 的对应值。
pub fn adjust_callback_url(callback_url: &str) -> String {
    if let Some(seclab_url_str) = get().seclab_url.as_deref().filter(|s| !s.is_empty()) {
        return rewrite_url(callback_url, seclab_url_str);
    }
    callback_url.to_string()
}

fn rewrite_url(callback_url: &str, seclab_url_str: &str) -> String {
    let target_url = match reqwest::Url::parse(seclab_url_str) {
        Ok(url) => url,
        Err(_) => return callback_url.to_string(),
    };

    let mut cb_url = match reqwest::Url::parse(callback_url) {
        Ok(url) => url,
        Err(_) => return callback_url.to_string(),
    };

    // 替换 scheme
    let _ = cb_url.set_scheme(target_url.scheme());
    // 替换 host
    let _ = cb_url.set_host(target_url.host_str());
    // 替换 port
    let _ = cb_url.set_port(target_url.port());

    cb_url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn default_stats_values_are_stable() {
        assert_eq!(stats_sample_interval(), Duration::from_secs(60));
        assert_eq!(stats_retention_hours(), 12);
    }

    #[test]
    fn dev_paths_use_expected_suffixes() {
        let compose_dir = compose_root_dir();
        let data_dir_path = data_dir();
        assert!(compose_dir.ends_with("data/compose"));
        assert!(data_dir_path.ends_with("database"));
    }

    #[test]
    fn test_rewrite_url() {
        // 1. 正常替换 host, port, scheme
        let orig = "https://10.0.0.43:7310/api/v1/runtime/heartbeat";
        let target = "http://10.121.7.7:8888";
        assert_eq!(
            rewrite_url(orig, target),
            "http://10.121.7.7:8888/api/v1/runtime/heartbeat"
        );

        // 2. 目标无 port，应清除原 port
        let target_no_port = "https://seclab-server.local";
        assert_eq!(
            rewrite_url(orig, target_no_port),
            "https://seclab-server.local/api/v1/runtime/heartbeat"
        );

        // 3. 无效 target url 应返回原样
        assert_eq!(rewrite_url(orig, "invalid-url"), orig);
    }
}
