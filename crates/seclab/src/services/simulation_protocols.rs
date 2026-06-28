//! 协议仿真能力注册表：集中维护主控当前认识的仿真协议。

/// 单个仿真协议的主控能力声明。
#[derive(Debug, Clone, Copy)]
pub struct SimulationProtocolCapability {
    /// 协议标识，必须与规则中的 `protocol` 字段一致。
    pub protocol: &'static str,
    /// 面向用户的协议名称。
    pub label: &'static str,
    /// 常见默认端口，用于 UI 和规则导入校验参考。
    pub default_port: u16,
    /// 主控是否允许把该协议下发到 Agent 部署。
    pub deployable: bool,
    /// 当前自定义规则表单是否支持创建该协议规则。
    pub custom_rule_creatable: bool,
    /// 审计日志中该协议可能产生的事件类型。
    pub event_types: &'static [&'static str],
}

const HTTP_EVENT_TYPES: &[&str] = &["connection", "http_request", "exploit_attempt"];
const REDIS_EVENT_TYPES: &[&str] = &["connection", "redis_command", "exploit_attempt"];
const SMTP_EVENT_TYPES: &[&str] = &[
    "connection",
    "auth_attempt",
    "smtp_command",
    "exploit_attempt",
];
const POP3_EVENT_TYPES: &[&str] = &[
    "connection",
    "auth_attempt",
    "pop3_command",
    "exploit_attempt",
];
const IMAP_EVENT_TYPES: &[&str] = &[
    "connection",
    "auth_attempt",
    "imap_command",
    "exploit_attempt",
];
const SSH_EVENT_TYPES: &[&str] = &["connection", "auth_attempt", "exploit_attempt"];
const FTP_EVENT_TYPES: &[&str] = &[
    "connection",
    "auth_attempt",
    "ftp_command",
    "exploit_attempt",
];
const RDP_EVENT_TYPES: &[&str] = &["connection", "rdp_negotiation", "exploit_attempt"];

const SIMULATION_PROTOCOLS: &[SimulationProtocolCapability] = &[
    SimulationProtocolCapability {
        protocol: "http",
        label: "HTTP",
        default_port: 8080,
        deployable: true,
        custom_rule_creatable: true,
        event_types: HTTP_EVENT_TYPES,
    },
    SimulationProtocolCapability {
        protocol: "redis",
        label: "Redis",
        default_port: 6379,
        deployable: true,
        custom_rule_creatable: false,
        event_types: REDIS_EVENT_TYPES,
    },
    SimulationProtocolCapability {
        protocol: "smtp",
        label: "SMTP",
        default_port: 25,
        deployable: true,
        custom_rule_creatable: false,
        event_types: SMTP_EVENT_TYPES,
    },
    SimulationProtocolCapability {
        protocol: "pop3",
        label: "POP3",
        default_port: 110,
        deployable: true,
        custom_rule_creatable: false,
        event_types: POP3_EVENT_TYPES,
    },
    SimulationProtocolCapability {
        protocol: "imap",
        label: "IMAP",
        default_port: 143,
        deployable: true,
        custom_rule_creatable: false,
        event_types: IMAP_EVENT_TYPES,
    },
    SimulationProtocolCapability {
        protocol: "ssh",
        label: "SSH",
        default_port: 22,
        deployable: true,
        custom_rule_creatable: false,
        event_types: SSH_EVENT_TYPES,
    },
    SimulationProtocolCapability {
        protocol: "ftp",
        label: "FTP",
        default_port: 21,
        deployable: true,
        custom_rule_creatable: false,
        event_types: FTP_EVENT_TYPES,
    },
    SimulationProtocolCapability {
        protocol: "rdp",
        label: "RDP",
        default_port: 3389,
        deployable: true,
        custom_rule_creatable: false,
        event_types: RDP_EVENT_TYPES,
    },
];

/// 返回主控已知的全部仿真协议能力。
pub fn list_simulation_protocols() -> &'static [SimulationProtocolCapability] {
    SIMULATION_PROTOCOLS
}

/// 查询指定协议的能力声明。
pub fn get_simulation_protocol(protocol: &str) -> Option<&'static SimulationProtocolCapability> {
    SIMULATION_PROTOCOLS
        .iter()
        .find(|item| item.protocol == protocol)
}

/// 判断协议是否可被部署到 Agent。
pub fn is_deployable_protocol(protocol: &str) -> bool {
    get_simulation_protocol(protocol)
        .map(|item| item.deployable)
        .unwrap_or(false)
}

/// 判断当前自定义规则入口是否支持创建该协议规则。
pub fn is_custom_rule_protocol(protocol: &str) -> bool {
    get_simulation_protocol(protocol)
        .map(|item| item.custom_rule_creatable)
        .unwrap_or(false)
}

/// 返回当前可部署协议的错误描述。
pub fn deployable_protocols_label() -> String {
    SIMULATION_PROTOCOLS
        .iter()
        .filter(|item| item.deployable)
        .map(|item| item.label)
        .collect::<Vec<_>>()
        .join(", ")
}

/// 返回当前自定义规则入口支持的协议错误描述。
pub fn custom_rule_protocols_label() -> String {
    SIMULATION_PROTOCOLS
        .iter()
        .filter(|item| item.custom_rule_creatable)
        .map(|item| item.label)
        .collect::<Vec<_>>()
        .join(", ")
}
