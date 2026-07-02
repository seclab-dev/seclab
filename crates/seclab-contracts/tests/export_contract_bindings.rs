//! 导出前端使用的 TypeScript 契约绑定。

use seclab_contracts::{
    api::{ApiResponse, ErrorCode},
    auth::{AuthBody, AuthSession, AuthUser},
    logging::{PlatformLog, PlatformLogList, PlatformLogQuery},
    notification::{
        NotificationBatchDeletePayload, NotificationCreatePayload, NotificationLevel,
        NotificationList, NotificationQuery, NotificationRecord,
    },
    process::{
        NetworkConnection, NetworkSnapshot, NetworkSummary, ProcessItem, ProcessManagerActiveView,
        ProcessManagerClientMessage, ProcessManagerError, ProcessManagerServerMessage,
        ProcessManagerSignalRequest, ProcessSnapshot, SignalResult,
    },
    runtime_logs::{RuntimeLogFile, RuntimeLogLine, RuntimeLogQuery, RuntimeLogQueryResult},
    seclab::{SeclabNetworkConfig, SeclabNetworkUpdateResult},
    simulation::{
        Credential, HttpExploitPathConfig, MailCredential, MailCustomResponse, MailMessage,
        RedisCommandResponse, SimFtpConfig, SimHttpConfig, SimImapConfig, SimPop3Config,
        SimRdpConfig, SimRedisConfig, SimSmtpConfig, SimSshConfig, SimulationConfig,
        SimulationEventType, SimulationProtocol, SimulationProtocolCapability,
    },
    telemetry::{LogModule, LogStatus},
    types::HostSystemSummary,
};
use std::path::Path;
use ts_rs::{Config, Dummy, TS};

fn export_type<T: TS + 'static>(cfg: &Config) {
    T::export_all(cfg).expect("failed to export TypeScript binding");
}

#[test]
fn export_contract_bindings() {
    let mut cfg = Config::from_env();
    if std::env::var_os("TS_RS_EXPORT_DIR").is_none() {
        cfg = cfg.with_out_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/ts-rs-bindings/seclab-contracts-test"),
        );
    }

    export_type::<ErrorCode>(&cfg);
    export_type::<ApiResponse<Dummy>>(&cfg);
    export_type::<AuthUser>(&cfg);
    export_type::<AuthSession>(&cfg);
    export_type::<AuthBody>(&cfg);
    export_type::<SeclabNetworkConfig>(&cfg);
    export_type::<SeclabNetworkUpdateResult>(&cfg);
    export_type::<LogStatus>(&cfg);
    export_type::<LogModule>(&cfg);
    export_type::<PlatformLog>(&cfg);
    export_type::<PlatformLogQuery>(&cfg);
    export_type::<PlatformLogList>(&cfg);
    export_type::<NotificationLevel>(&cfg);
    export_type::<NotificationRecord>(&cfg);
    export_type::<NotificationCreatePayload>(&cfg);
    export_type::<NotificationQuery>(&cfg);
    export_type::<NotificationBatchDeletePayload>(&cfg);
    export_type::<NotificationList>(&cfg);
    export_type::<ProcessItem>(&cfg);
    export_type::<ProcessManagerSignalRequest>(&cfg);
    export_type::<ProcessManagerActiveView>(&cfg);
    export_type::<ProcessManagerClientMessage>(&cfg);
    export_type::<ProcessSnapshot>(&cfg);
    export_type::<NetworkConnection>(&cfg);
    export_type::<NetworkSummary>(&cfg);
    export_type::<NetworkSnapshot>(&cfg);
    export_type::<SignalResult>(&cfg);
    export_type::<ProcessManagerError>(&cfg);
    export_type::<ProcessManagerServerMessage>(&cfg);
    export_type::<HostSystemSummary>(&cfg);
    export_type::<RuntimeLogFile>(&cfg);
    export_type::<RuntimeLogQuery>(&cfg);
    export_type::<RuntimeLogLine>(&cfg);
    export_type::<RuntimeLogQueryResult>(&cfg);
    export_type::<SimulationProtocol>(&cfg);
    export_type::<SimulationEventType>(&cfg);
    export_type::<SimulationProtocolCapability>(&cfg);
    export_type::<HttpExploitPathConfig>(&cfg);
    export_type::<SimHttpConfig>(&cfg);
    export_type::<RedisCommandResponse>(&cfg);
    export_type::<SimRedisConfig>(&cfg);
    export_type::<MailCredential>(&cfg);
    export_type::<MailMessage>(&cfg);
    export_type::<MailCustomResponse>(&cfg);
    export_type::<SimSmtpConfig>(&cfg);
    export_type::<SimPop3Config>(&cfg);
    export_type::<SimImapConfig>(&cfg);
    export_type::<Credential>(&cfg);
    export_type::<SimSshConfig>(&cfg);
    export_type::<SimFtpConfig>(&cfg);
    export_type::<SimRdpConfig>(&cfg);
    export_type::<SimulationConfig>(&cfg);
}
