//! 协议仿真共享契约：统一协议标识、能力元数据、事件类型和规则配置 schema。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;
use ts_rs::TS;

/// 协议仿真支持的协议标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export_to = "simulation/")]
pub enum SimulationProtocol {
    /// HTTP 协议。
    Http,
    /// Redis 协议。
    Redis,
    /// SMTP 协议。
    Smtp,
    /// POP3 协议。
    Pop3,
    /// IMAP 协议。
    Imap,
    /// SSH 协议。
    Ssh,
    /// FTP 协议。
    Ftp,
    /// RDP 协议。
    Rdp,
}

impl SimulationProtocol {
    /// 返回用于 API、数据库和规则包的稳定小写协议标识。
    pub const fn as_str(self) -> &'static str {
        match self {
            SimulationProtocol::Http => "http",
            SimulationProtocol::Redis => "redis",
            SimulationProtocol::Smtp => "smtp",
            SimulationProtocol::Pop3 => "pop3",
            SimulationProtocol::Imap => "imap",
            SimulationProtocol::Ssh => "ssh",
            SimulationProtocol::Ftp => "ftp",
            SimulationProtocol::Rdp => "rdp",
        }
    }
}

impl FromStr for SimulationProtocol {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "http" => Ok(SimulationProtocol::Http),
            "redis" => Ok(SimulationProtocol::Redis),
            "smtp" => Ok(SimulationProtocol::Smtp),
            "pop3" => Ok(SimulationProtocol::Pop3),
            "imap" => Ok(SimulationProtocol::Imap),
            "ssh" => Ok(SimulationProtocol::Ssh),
            "ftp" => Ok(SimulationProtocol::Ftp),
            "rdp" => Ok(SimulationProtocol::Rdp),
            _ => Err(format!("unsupported simulation protocol: {value}")),
        }
    }
}

/// 协议仿真审计日志事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/")]
pub enum SimulationEventType {
    /// 客户端建立连接。
    Connection,
    /// HTTP 请求事件。
    HttpRequest,
    /// Redis 命令事件。
    RedisCommand,
    /// 认证尝试事件。
    AuthAttempt,
    /// SMTP 命令事件。
    SmtpCommand,
    /// POP3 命令事件。
    Pop3Command,
    /// IMAP 命令事件。
    ImapCommand,
    /// FTP 命令事件。
    FtpCommand,
    /// RDP 协商事件。
    RdpNegotiation,
    /// 漏洞或诱捕触发事件。
    ExploitAttempt,
}

impl SimulationEventType {
    /// 返回用于日志存储和 API 输出的稳定事件标识。
    pub const fn as_str(self) -> &'static str {
        match self {
            SimulationEventType::Connection => "connection",
            SimulationEventType::HttpRequest => "http_request",
            SimulationEventType::RedisCommand => "redis_command",
            SimulationEventType::AuthAttempt => "auth_attempt",
            SimulationEventType::SmtpCommand => "smtp_command",
            SimulationEventType::Pop3Command => "pop3_command",
            SimulationEventType::ImapCommand => "imap_command",
            SimulationEventType::FtpCommand => "ftp_command",
            SimulationEventType::RdpNegotiation => "rdp_negotiation",
            SimulationEventType::ExploitAttempt => "exploit_attempt",
        }
    }
}

impl FromStr for SimulationEventType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "connection" => Ok(SimulationEventType::Connection),
            "http_request" => Ok(SimulationEventType::HttpRequest),
            "redis_command" => Ok(SimulationEventType::RedisCommand),
            "auth_attempt" => Ok(SimulationEventType::AuthAttempt),
            "smtp_command" => Ok(SimulationEventType::SmtpCommand),
            "pop3_command" => Ok(SimulationEventType::Pop3Command),
            "imap_command" => Ok(SimulationEventType::ImapCommand),
            "ftp_command" => Ok(SimulationEventType::FtpCommand),
            "rdp_negotiation" => Ok(SimulationEventType::RdpNegotiation),
            "exploit_attempt" => Ok(SimulationEventType::ExploitAttempt),
            _ => Err(format!("unsupported simulation event type: {value}")),
        }
    }
}

/// 单个仿真协议的能力声明。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "simulation/")]
pub struct SimulationProtocolCapability {
    /// 协议标识。
    pub protocol: SimulationProtocol,
    /// 面向用户的协议名称。
    pub label: String,
    /// 常见默认端口。
    pub default_port: u16,
    /// 是否允许部署到 Agent。
    pub deployable: bool,
    /// 是否允许通过控制台手动创建自定义规则。
    pub custom_rule_creatable: bool,
    /// 该协议可能产生的审计事件类型。
    pub event_types: Vec<SimulationEventType>,
}

/// HTTP 仿真漏洞路径配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct HttpExploitPathConfig {
    /// 触发路径。
    pub path: String,
    /// 触发 HTTP 方法；为空时匹配任意方法。
    pub trigger_method: Option<String>,
    /// 命中路径后的响应状态码。
    pub response_status: u16,
    /// 命中路径后的响应正文。
    pub response_body: String,
    /// 命中路径后的附加响应头。
    pub response_headers: Option<BTreeMap<String, String>>,
}

/// HTTP 协议仿真配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct SimHttpConfig {
    /// Server 响应头。
    pub server_header: Option<String>,
    /// 全局响应头。
    pub headers: Option<BTreeMap<String, String>>,
    /// 默认静态 HTML 响应。
    pub html: Option<String>,
    /// 漏洞路径触发配置。
    pub exploit_paths: Option<Vec<HttpExploitPathConfig>>,
}

/// Redis 自定义命令响应配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct RedisCommandResponse {
    /// 匹配的 Redis 命令名称。
    pub command: String,
    /// 参数内容包含条件。
    pub args_contains: Option<Vec<String>>,
    /// 返回给客户端的 RESP 响应。
    pub response: String,
    /// 命中后上报的事件类型。
    pub event_type: Option<String>,
}

/// Redis 协议仿真配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct SimRedisConfig {
    /// 连接欢迎信息。
    pub banner: Option<String>,
    /// 是否要求 AUTH。
    pub require_auth: Option<bool>,
    /// AUTH 密码。
    pub password: Option<String>,
    /// INFO 命令返回的服务端信息。
    pub server_info: Option<BTreeMap<String, String>>,
    /// 静态键值数据。
    pub keys: Option<BTreeMap<String, String>>,
    /// 自定义命令响应。
    pub command_responses: Option<Vec<RedisCommandResponse>>,
}

/// 邮件协议认证凭据。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct MailCredential {
    /// 用户名。
    pub username: String,
    /// 密码。
    pub password: String,
    /// 展示名称。
    pub display_name: Option<String>,
}

/// 邮件协议静态邮件数据。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct MailMessage {
    /// 邮件 UID。
    pub uid: Option<String>,
    /// 发件人地址。
    pub from: String,
    /// 收件人地址列表。
    pub to: Vec<String>,
    /// 邮件主题。
    pub subject: String,
    /// 邮件日期。
    pub date: Option<String>,
    /// 邮件正文。
    pub body: String,
    /// 邮件标记。
    pub flags: Option<Vec<String>>,
}

/// IMAP 邮箱到邮件列表的映射。
pub type Mailboxes = BTreeMap<String, Vec<MailMessage>>;

/// 邮件协议自定义命令响应配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct MailCustomResponse {
    /// 匹配的命令名称。
    pub command: String,
    /// 参数内容包含条件。
    pub args_contains: Option<Vec<String>>,
    /// 返回给客户端的响应内容。
    pub response: String,
    /// 命中后上报的事件类型。
    pub event_type: Option<String>,
}

/// SMTP 协议仿真配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct SimSmtpConfig {
    /// SMTP 欢迎 banner。
    pub banner: Option<String>,
    /// EHLO/HELO 主机名。
    pub hostname: Option<String>,
    /// 是否要求认证。
    pub require_auth: Option<bool>,
    /// 可匹配的认证凭据。
    pub credentials: Option<Vec<MailCredential>>,
    /// 服务端能力列表。
    pub capabilities: Option<Vec<String>>,
    /// 接受的收件人地址列表。
    pub accepted_recipients: Option<Vec<String>>,
    /// 自定义命令响应。
    pub custom_responses: Option<Vec<MailCustomResponse>>,
}

/// POP3 协议仿真配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct SimPop3Config {
    /// POP3 欢迎 banner。
    pub banner: Option<String>,
    /// 是否要求认证。
    pub require_auth: Option<bool>,
    /// 可匹配的认证凭据。
    pub credentials: Option<Vec<MailCredential>>,
    /// 服务端能力列表。
    pub capabilities: Option<Vec<String>>,
    /// 静态邮件列表。
    pub messages: Option<Vec<MailMessage>>,
    /// 自定义命令响应。
    pub custom_responses: Option<Vec<MailCustomResponse>>,
}

/// IMAP 协议仿真配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct SimImapConfig {
    /// IMAP 欢迎 banner。
    pub banner: Option<String>,
    /// 是否要求认证。
    pub require_auth: Option<bool>,
    /// 可匹配的认证凭据。
    pub credentials: Option<Vec<MailCredential>>,
    /// 服务端能力列表。
    pub capabilities: Option<Vec<String>>,
    /// 邮箱到邮件列表的映射。
    pub mailboxes: Option<Mailboxes>,
    /// 兼容旧规则的默认邮件列表。
    pub messages: Option<Vec<MailMessage>>,
    /// 自定义命令响应。
    pub custom_responses: Option<Vec<MailCustomResponse>>,
}

/// 用户名密码凭据。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/")]
pub struct Credential {
    /// 用户名。
    pub username: String,
    /// 密码。
    pub password: String,
}

/// SSH 协议仿真配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct SimSshConfig {
    /// SSH 版本 banner。
    pub banner: Option<String>,
    /// 弱口令凭据列表。
    pub credentials: Option<Vec<Credential>>,
}

/// FTP 协议仿真配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct SimFtpConfig {
    /// FTP 欢迎 banner。
    pub banner: Option<String>,
    /// 弱口令凭据列表。
    pub credentials: Option<Vec<Credential>>,
    /// SYST 命令返回的服务端系统名。
    pub server_name: Option<String>,
    /// 是否允许匿名登录。
    pub allow_anonymous: Option<bool>,
}

/// RDP 协议仿真配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "simulation/", optional_fields)]
pub struct SimRdpConfig {
    /// X.224/RDP 协商 flags。
    pub flags: Option<u32>,
    /// 弱口令凭据列表，保留给后续更高保真交互使用。
    pub credentials: Option<Vec<Credential>>,
}

/// 已按协议解析完成的仿真规则配置。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "protocol", content = "config", rename_all = "lowercase")]
#[ts(export_to = "simulation/")]
pub enum SimulationConfig {
    /// HTTP 协议配置。
    Http(SimHttpConfig),
    /// Redis 协议配置。
    Redis(SimRedisConfig),
    /// SMTP 协议配置。
    Smtp(SimSmtpConfig),
    /// POP3 协议配置。
    Pop3(SimPop3Config),
    /// IMAP 协议配置。
    Imap(SimImapConfig),
    /// SSH 协议配置。
    Ssh(SimSshConfig),
    /// FTP 协议配置。
    Ftp(SimFtpConfig),
    /// RDP 协议配置。
    Rdp(SimRdpConfig),
}

/// 按协议解析仿真配置 JSON 值。
pub fn parse_simulation_config(
    protocol: SimulationProtocol,
    value: serde_json::Value,
) -> Result<SimulationConfig, serde_json::Error> {
    match protocol {
        SimulationProtocol::Http => serde_json::from_value(value).map(SimulationConfig::Http),
        SimulationProtocol::Redis => serde_json::from_value(value).map(SimulationConfig::Redis),
        SimulationProtocol::Smtp => serde_json::from_value(value).map(SimulationConfig::Smtp),
        SimulationProtocol::Pop3 => serde_json::from_value(value).map(SimulationConfig::Pop3),
        SimulationProtocol::Imap => serde_json::from_value(value).map(SimulationConfig::Imap),
        SimulationProtocol::Ssh => serde_json::from_value(value).map(SimulationConfig::Ssh),
        SimulationProtocol::Ftp => serde_json::from_value(value).map(SimulationConfig::Ftp),
        SimulationProtocol::Rdp => serde_json::from_value(value).map(SimulationConfig::Rdp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simulation_config_accepts_minimal_protocol_configs() {
        let cases = [
            (
                SimulationProtocol::Http,
                serde_json::json!({"server_header":"nginx","exploit_paths":[]}),
            ),
            (
                SimulationProtocol::Redis,
                serde_json::json!({"require_auth":true,"password":"redis123","keys":{"session":"admin"}}),
            ),
            (
                SimulationProtocol::Smtp,
                serde_json::json!({"hostname":"mail.seclab.local","credentials":[{"username":"admin","password":"password"}]}),
            ),
            (
                SimulationProtocol::Pop3,
                serde_json::json!({"messages":[{"from":"alerts@seclab.local","to":["admin@seclab.local"],"subject":"Alert","body":"Body"}]}),
            ),
            (
                SimulationProtocol::Imap,
                serde_json::json!({"mailboxes":{"INBOX":[{"from":"alerts@seclab.local","to":["admin@seclab.local"],"subject":"Alert","body":"Body"}]}}),
            ),
            (
                SimulationProtocol::Ssh,
                serde_json::json!({"banner":"SSH-2.0-OpenSSH_8.9","credentials":[{"username":"root","password":"toor"}]}),
            ),
            (
                SimulationProtocol::Ftp,
                serde_json::json!({"server_name":"UNIX Type: L8","allow_anonymous":false,"credentials":[{"username":"admin","password":"password"}]}),
            ),
            (
                SimulationProtocol::Rdp,
                serde_json::json!({"flags":1,"credentials":[{"username":"administrator","password":"Password123"}]}),
            ),
        ];

        for (protocol, config) in cases {
            parse_simulation_config(protocol, config).unwrap_or_else(|err| {
                panic!("{} config should be valid: {}", protocol.as_str(), err)
            });
        }
    }

    #[test]
    fn parse_simulation_config_rejects_type_mismatch() {
        assert!(
            parse_simulation_config(SimulationProtocol::Redis, serde_json::json!({"keys":[]}))
                .is_err()
        );
        assert!(
            parse_simulation_config(SimulationProtocol::Rdp, serde_json::json!({"flags":"tls"}))
                .is_err()
        );
    }
}
