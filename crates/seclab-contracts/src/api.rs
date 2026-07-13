//! API 共享契约：统一响应结构与错误码定义。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 统一业务错误码，供 seclab、agent 与 frontend 对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export_to = "api/")]
pub enum ErrorCode {
    AuthUnauthorized,
    AuthForbidden,
    AuthMissingCredentials,
    AuthWrongCredentials,
    AuthInvalidToken,
    AuthTokenExpired,
    AuthTokenCreationFailed,
    AuthInvalidPassword,
    AuthWrongOldPassword,
    AuthUsernameExists,
    AuthClientVersionMismatch,
    AgentUnavailable,
    AgentNotFound,
    AgentPermissionDenied,
    AgentRefused,
    AgentTimeout,
    AgentRequestFailed,
    NodeNotFound,
    NodeInvalidTarget,
    NodeUnavailable,
    NodeAlreadyExists,
    NodeDeployFailed,
    NodePrecheckFailed,
    RuntimeSessionNotFound,
    RuntimeLeaseMismatch,
    RuntimeRegistrationFailed,
    RuntimeCertificateRotationFailed,
    DockerUnavailable,
    DockerOperationFailed,
    DockerContainerNotFound,
    SuiteWorkloadPortUnavailable,
    FileInvalidPath,
    FileNotFound,
    FileAlreadyExists,
    FileMissingName,
    FileUploadInvalid,
    FileOperationFailed,
    TaskNotFound,
    TaskInvalidSchedule,
    TaskExecutionFailed,
    TaskRevisionConflict,
    SeclabInvalidConfig,
    SeclabPortUnavailable,
    ValidationFailed,
    BadRequest,
    InternalServerError,
    DatabaseError,
    ExternalRequestFailed,
}

impl ErrorCode {
    /// 返回稳定字符串，便于作为跨端协议字段传输。
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::AuthUnauthorized => "AUTH_UNAUTHORIZED",
            ErrorCode::AuthForbidden => "AUTH_FORBIDDEN",
            ErrorCode::AuthMissingCredentials => "AUTH_MISSING_CREDENTIALS",
            ErrorCode::AuthWrongCredentials => "AUTH_WRONG_CREDENTIALS",
            ErrorCode::AuthInvalidToken => "AUTH_INVALID_TOKEN",
            ErrorCode::AuthTokenExpired => "AUTH_TOKEN_EXPIRED",
            ErrorCode::AuthTokenCreationFailed => "AUTH_TOKEN_CREATION_FAILED",
            ErrorCode::AuthInvalidPassword => "AUTH_INVALID_PASSWORD",
            ErrorCode::AuthWrongOldPassword => "AUTH_WRONG_OLD_PASSWORD",
            ErrorCode::AuthUsernameExists => "AUTH_USERNAME_EXISTS",
            ErrorCode::AuthClientVersionMismatch => "AUTH_CLIENT_VERSION_MISMATCH",
            ErrorCode::AgentUnavailable => "AGENT_UNAVAILABLE",
            ErrorCode::AgentNotFound => "AGENT_NOT_FOUND",
            ErrorCode::AgentPermissionDenied => "AGENT_PERMISSION_DENIED",
            ErrorCode::AgentRefused => "AGENT_REFUSED",
            ErrorCode::AgentTimeout => "AGENT_TIMEOUT",
            ErrorCode::AgentRequestFailed => "AGENT_REQUEST_FAILED",
            ErrorCode::NodeNotFound => "NODE_NOT_FOUND",
            ErrorCode::NodeInvalidTarget => "NODE_INVALID_TARGET",
            ErrorCode::NodeUnavailable => "NODE_UNAVAILABLE",
            ErrorCode::NodeAlreadyExists => "NODE_ALREADY_EXISTS",
            ErrorCode::NodeDeployFailed => "NODE_DEPLOY_FAILED",
            ErrorCode::NodePrecheckFailed => "NODE_PRECHECK_FAILED",
            ErrorCode::RuntimeSessionNotFound => "RUNTIME_SESSION_NOT_FOUND",
            ErrorCode::RuntimeLeaseMismatch => "RUNTIME_LEASE_MISMATCH",
            ErrorCode::RuntimeRegistrationFailed => "RUNTIME_REGISTRATION_FAILED",
            ErrorCode::RuntimeCertificateRotationFailed => "RUNTIME_CERTIFICATE_ROTATION_FAILED",
            ErrorCode::DockerUnavailable => "DOCKER_UNAVAILABLE",
            ErrorCode::DockerOperationFailed => "DOCKER_OPERATION_FAILED",
            ErrorCode::DockerContainerNotFound => "DOCKER_CONTAINER_NOT_FOUND",
            ErrorCode::SuiteWorkloadPortUnavailable => "SUITE_WORKLOAD_PORT_UNAVAILABLE",
            ErrorCode::FileInvalidPath => "FILE_INVALID_PATH",
            ErrorCode::FileNotFound => "FILE_NOT_FOUND",
            ErrorCode::FileAlreadyExists => "FILE_ALREADY_EXISTS",
            ErrorCode::FileMissingName => "FILE_MISSING_NAME",
            ErrorCode::FileUploadInvalid => "FILE_UPLOAD_INVALID",
            ErrorCode::FileOperationFailed => "FILE_OPERATION_FAILED",
            ErrorCode::TaskNotFound => "TASK_NOT_FOUND",
            ErrorCode::TaskInvalidSchedule => "TASK_INVALID_SCHEDULE",
            ErrorCode::TaskExecutionFailed => "TASK_EXECUTION_FAILED",
            ErrorCode::TaskRevisionConflict => "TASK_REVISION_CONFLICT",
            ErrorCode::SeclabInvalidConfig => "SECLAB_INVALID_CONFIG",
            ErrorCode::SeclabPortUnavailable => "SECLAB_PORT_UNAVAILABLE",
            ErrorCode::ValidationFailed => "VALIDATION_FAILED",
            ErrorCode::BadRequest => "BAD_REQUEST",
            ErrorCode::InternalServerError => "INTERNAL_SERVER_ERROR",
            ErrorCode::DatabaseError => "DATABASE_ERROR",
            ErrorCode::ExternalRequestFailed => "EXTERNAL_REQUEST_FAILED",
        }
    }
}

/// 统一封装成功状态、消息与可选数据的响应体。
///
/// 用于封装所有 HTTP API 的返回数据，提供统一的跨端格式。
#[derive(Serialize, Deserialize, Debug, TS)]
#[ts(export_to = "api/", optional_fields)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub code: u16,
    pub message: String,
    #[serde(rename = "messageKey", skip_serializing_if = "Option::is_none")]
    pub message_key: Option<String>,
    #[serde(rename = "errorCode", skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

/// `ApiResponse` 的构建器。
pub struct ApiResponseBuilder<T> {
    success: bool,
    code: u16,
    message: String,
    message_key: Option<String>,
    error_code: Option<ErrorCode>,
    data: Option<T>,
}

impl<T> ApiResponseBuilder<T> {
    /// 创建一个新的 `ApiResponseBuilder`，并带有默认值。
    pub fn new() -> Self {
        Self {
            success: true,
            code: 200,
            message: String::new(),
            message_key: None,
            error_code: None,
            data: None,
        }
    }

    /// 设置 `success` 字段。
    pub fn success(mut self, success: bool) -> Self {
        self.success = success;
        self
    }

    /// 设置 HTTP 状态码。
    pub fn code(mut self, code: u16) -> Self {
        self.code = code;
        self
    }

    /// 设置响应消息。
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// 设置响应 i18n 消息 key。
    pub fn message_key(mut self, message_key: impl Into<String>) -> Self {
        self.message_key = Some(message_key.into());
        self
    }

    /// 设置统一业务错误码。
    pub fn error_code(mut self, error_code: ErrorCode) -> Self {
        self.error_code = Some(error_code);
        self
    }

    /// 设置响应数据。
    pub fn data(mut self, data: T) -> Self {
        self.data = Some(data);
        self
    }

    /// 构建最终的 `ApiResponse<T>` 实例。
    pub fn build(self) -> ApiResponse<T> {
        ApiResponse {
            success: self.success,
            code: self.code,
            message: self.message,
            message_key: self.message_key,
            error_code: self.error_code,
            data: self.data,
        }
    }
}

impl<T> Default for ApiResponseBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ApiResponse<T> {
    /// 返回一个新的 `ApiResponseBuilder` 实例。
    pub fn builder() -> ApiResponseBuilder<T> {
        ApiResponseBuilder::new()
    }

    /// 快速构建一个失败的 `ApiResponse`。
    pub fn error(message: &str, raw_data: T, code: u16) -> Self {
        ApiResponse {
            success: false,
            message: message.to_string(),
            message_key: None,
            error_code: None,
            data: Some(raw_data),
            code,
        }
    }

    /// 快速构建一个带业务错误码的失败响应。
    pub fn error_with_code(message: &str, error_code: ErrorCode, raw_data: T, code: u16) -> Self {
        ApiResponse {
            success: false,
            message: message.to_string(),
            message_key: None,
            error_code: Some(error_code),
            data: Some(raw_data),
            code,
        }
    }

    /// 快速构建一个成功的 `ApiResponse`。
    pub fn success(message: &str, raw_data: T, code: u16) -> Self {
        Self {
            success: true,
            code,
            message: message.to_string(),
            message_key: None,
            error_code: None,
            data: Some(raw_data),
        }
    }

    /// 快速构建一个成功的 `ApiResponse`，使用默认的 `code = 200`。
    pub fn success_with_raw(message: &str, raw_data: T) -> Self {
        Self {
            success: true,
            code: 200,
            message: message.to_string(),
            message_key: None,
            error_code: None,
            data: Some(raw_data),
        }
    }

    /// 快速构建一个失败的 `ApiResponse`，使用默认的 `code = 400`。
    pub fn error_with_raw(message: &str, raw_data: T) -> Self {
        Self {
            success: false,
            code: 400,
            message: message.to_string(),
            message_key: None,
            error_code: None,
            data: Some(raw_data),
        }
    }

    /// 设置响应的 HTTP 状态码。
    pub fn set_code(mut self, code: u16) -> Self {
        self.code = code;
        self
    }
}

impl ApiResponse<()> {
    /// 快速构建一个成功的、不带 `data` 的 `ApiResponse`。
    pub fn ok(message: &str) -> ApiResponse<()> {
        ApiResponse {
            success: true,
            code: 200,
            message: message.to_string(),
            message_key: None,
            error_code: None,
            data: None,
        }
    }

    /// 快速构建一个失败的、不带 `data` 的 `ApiResponse`。
    ///
    /// 注意：此处的 `success` 字段为 `true`，为保持兼容性暂不修改。
    pub fn fail(message: &str) -> ApiResponse<()> {
        ApiResponse {
            success: true,
            code: 400,
            message: message.to_string(),
            message_key: None,
            error_code: None,
            data: None,
        }
    }

    /// 快速构建一个成功的、不带 `data` 的 `ApiResponse`，并指定状态码。
    pub fn success_msg(message: &str, code: u16) -> ApiResponse<()> {
        ApiResponse {
            success: true,
            code,
            message: message.to_string(),
            message_key: None,
            error_code: None,
            data: None,
        }
    }

    /// 快速构建一个失败的、不带 `data` 的 `ApiResponse`，并指定状态码。
    pub fn error_msg(message: &str, code: u16) -> ApiResponse<()> {
        ApiResponse {
            success: false,
            code,
            message: message.to_string(),
            message_key: None,
            error_code: None,
            data: None,
        }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    /// 将统一响应封装为 Axum JSON 响应。
    fn into_response(self) -> Response {
        let status_code = match StatusCode::from_u16(self.code) {
            Ok(code) => code,
            Err(_) => {
                if self.success {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_REQUEST
                }
            }
        };
        (status_code, Json(self)).into_response()
    }
}
