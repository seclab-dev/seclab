//! 导出前端使用的 TypeScript 契约绑定。

use seclab_contracts::{
    api::{ApiResponse, ErrorCode},
    auth::{AuthBody, AuthSession, AuthUser},
    files::{FileDocument, FileSaveResult, UpdateFileContentRequest},
    logging::{PlatformLog, PlatformLogList, PlatformLogQuery},
    monitoring::{SystemMonitoringOverview, SystemMonitoringSeriesPage, SystemMonitoringSettings},
    notification::{
        NotificationBatchDeletePayload, NotificationCreatePayload, NotificationLevel,
        NotificationList, NotificationQuery, NotificationRecord,
    },
    process::{
        NetworkConnectionListPage, ProcessActionRequest, ProcessForceKillConfirmation,
        ProcessListPage, ProcessSignalResult,
    },
    runtime_logs::{RuntimeLogFile, RuntimeLogLine, RuntimeLogQuery, RuntimeLogQueryResult},
    scheduled_tasks::{
        CreateScheduledTaskBatchRequest, CreateScheduledTaskMigrationRequest,
        CreateScheduledTaskRequest, ScheduledTaskBatch, ScheduledTaskDetail, ScheduledTaskListPage,
        ScheduledTaskOperation, ScheduledTaskRun, ScheduledTaskRunOutput, ScheduledTaskRunPage,
        UpdateScheduledTaskRequest, UpdateScheduledTaskStateRequest,
    },
    seclab::{SeclabNetworkConfig, SeclabNetworkUpdateResult},
    telemetry::{LogModule, LogStatus},
    terminal::{TerminalAccess, TerminalClientMessage, TerminalServerMessage},
};
use std::path::Path;
use ts_rs::{Config, Dummy, TS};

fn export_type<T: TS + 'static>(cfg: &Config) {
    T::export_all(cfg).expect("failed to export TypeScript binding");
}

fn export_config() -> Config {
    let mut cfg = Config::from_env();
    if std::env::var_os("TS_RS_EXPORT_DIR").is_none() {
        cfg = cfg.with_out_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/ts-rs-bindings/seclab-contracts-test"),
        );
    }
    cfg
}

#[test]
fn export_error_code_binding() {
    export_type::<ErrorCode>(&export_config());
}

#[test]
fn export_contract_bindings() {
    let cfg = export_config();

    export_type::<ErrorCode>(&cfg);
    export_type::<ApiResponse<Dummy>>(&cfg);
    export_type::<AuthUser>(&cfg);
    export_type::<AuthSession>(&cfg);
    export_type::<AuthBody>(&cfg);
    export_type::<FileDocument>(&cfg);
    export_type::<FileSaveResult>(&cfg);
    export_type::<UpdateFileContentRequest>(&cfg);
    export_type::<SeclabNetworkConfig>(&cfg);
    export_type::<SeclabNetworkUpdateResult>(&cfg);
    export_type::<LogStatus>(&cfg);
    export_type::<LogModule>(&cfg);
    export_type::<PlatformLog>(&cfg);
    export_type::<PlatformLogQuery>(&cfg);
    export_type::<PlatformLogList>(&cfg);
    export_type::<SystemMonitoringOverview>(&cfg);
    export_type::<SystemMonitoringSeriesPage>(&cfg);
    export_type::<SystemMonitoringSettings>(&cfg);
    export_type::<NotificationLevel>(&cfg);
    export_type::<NotificationRecord>(&cfg);
    export_type::<NotificationCreatePayload>(&cfg);
    export_type::<NotificationQuery>(&cfg);
    export_type::<NotificationBatchDeletePayload>(&cfg);
    export_type::<NotificationList>(&cfg);
    export_type::<ProcessListPage>(&cfg);
    export_type::<NetworkConnectionListPage>(&cfg);
    export_type::<ProcessActionRequest>(&cfg);
    export_type::<ProcessForceKillConfirmation>(&cfg);
    export_type::<ProcessSignalResult>(&cfg);
    export_type::<RuntimeLogFile>(&cfg);
    export_type::<RuntimeLogQuery>(&cfg);
    export_type::<RuntimeLogLine>(&cfg);
    export_type::<RuntimeLogQueryResult>(&cfg);
    export_type::<ScheduledTaskListPage>(&cfg);
    export_type::<ScheduledTaskDetail>(&cfg);
    export_type::<CreateScheduledTaskRequest>(&cfg);
    export_type::<UpdateScheduledTaskRequest>(&cfg);
    export_type::<UpdateScheduledTaskStateRequest>(&cfg);
    export_type::<CreateScheduledTaskMigrationRequest>(&cfg);
    export_type::<CreateScheduledTaskBatchRequest>(&cfg);
    export_type::<ScheduledTaskRun>(&cfg);
    export_type::<ScheduledTaskRunPage>(&cfg);
    export_type::<ScheduledTaskRunOutput>(&cfg);
    export_type::<ScheduledTaskOperation>(&cfg);
    export_type::<ScheduledTaskBatch>(&cfg);
    export_type::<TerminalAccess>(&cfg);
    export_type::<TerminalClientMessage>(&cfg);
    export_type::<TerminalServerMessage>(&cfg);
}
