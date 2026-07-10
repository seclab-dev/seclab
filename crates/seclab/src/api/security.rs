//! 安全设置 API：安全入口、账号凭证与密码复杂度管理。

use crate::api::auth::AuthenticatedAdmin;
use crate::models::system_config;
use crate::security::password::validate_password;
use crate::security::safe_entry_rules::{SafeEntryValidationError, validate_safe_entry_value};
use crate::services::user_service::hash_password;
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 安全配置响应。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettings {
    pub safe_entry: String,
    pub safe_entry_enabled: bool,
    pub password_complexity: bool,
}

/// 安全配置更新请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSecuritySettings {
    pub safe_entry: String,
    pub password_complexity: bool,
}

/// 修改密码请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePasswordPayload {
    pub new_password: String,
}

/// 修改用户名请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUsernamePayload {
    pub new_username: String,
}

/// 创建安全设置路由。
pub fn security_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/settings", get(get_settings))
        .route("/settings", put(update_settings))
        .route("/password", put(update_password))
        .route("/username", put(update_username))
}

async fn get_settings(
    State(state): State<Arc<AppState>>,
    _admin: AuthenticatedAdmin,
) -> ApiResult<impl IntoResponse> {
    let safe_entry = system_config::get_safe_entry_value(&state.metadata_db).await?;
    let payload = SecuritySettings {
        safe_entry_enabled: !safe_entry.is_empty(),
        safe_entry,
        password_complexity: system_config::password_complexity_enabled(&state.metadata_db).await?,
    };
    let mut body = ApiResponse::success_with_raw("Security settings loaded", payload);
    body.message_key = Some("api.security.settingsLoaded".to_string());
    Ok(body.into_response())
}

async fn update_settings(
    State(state): State<Arc<AppState>>,
    _admin: AuthenticatedAdmin,
    Json(payload): Json<UpdateSecuritySettings>,
) -> ApiResult<Response> {
    let safe_entry = payload.safe_entry.trim();
    if !safe_entry.is_empty() {
        validate_safe_entry_for_api(safe_entry)?;
    }
    let previous_safe_entry = system_config::get_safe_entry(&state.metadata_db).await?;
    system_config::update_security_settings(
        &state.metadata_db,
        safe_entry,
        payload.password_complexity,
    )
    .await?;
    let mut body = ApiResponse::ok("Security settings updated");
    body.message_key = Some("api.security.settingsUpdated".to_string());
    let mut resp = body.into_response();
    if previous_safe_entry.as_deref() != Some(safe_entry) {
        resp.headers_mut().append(
            header::SET_COOKIE,
            crate::security::safe_entry::clear_safe_entry_cookie()
                .parse()
                .expect("safe entry clearing cookie must be a valid header"),
        );
    }
    Ok(resp)
}

async fn update_password(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    Json(payload): Json<UpdatePasswordPayload>,
) -> ApiResult<impl IntoResponse> {
    let enforce_complexity = system_config::password_complexity_enabled(&state.metadata_db).await?;
    validate_password(&payload.new_password, enforce_complexity).map_err(|err| {
        ApiError::validation(err.to_string()).with_message_key("api.security.passwordInvalid")
    })?;
    let password_hash =
        hash_password(&payload.new_password).map_err(|err| ApiError::internal(err.to_string()))?;
    sqlx::query("UPDATE users SET password_hash = ?1 WHERE id = ?2")
        .bind(password_hash)
        .bind(admin.id)
        .execute(&state.metadata_db)
        .await?;
    let mut body = ApiResponse::ok("Password updated");
    body.message_key = Some("api.security.passwordUpdated".to_string());
    Ok(body.into_response())
}

async fn update_username(
    State(state): State<Arc<AppState>>,
    admin: AuthenticatedAdmin,
    Json(payload): Json<UpdateUsernamePayload>,
) -> ApiResult<impl IntoResponse> {
    let username = payload.new_username.trim();
    validate_username(username)?;
    sqlx::query("UPDATE users SET username = ?1 WHERE id = ?2")
        .bind(username)
        .bind(admin.id)
        .execute(&state.metadata_db)
        .await
        .map_err(|err| {
            if matches!(err, sqlx::Error::Database(ref db_err) if db_err.is_unique_violation()) {
                ApiError::from(crate::errors::AuthError::UsernameExists)
            } else {
                ApiError::from(err)
            }
        })?;
    let mut body = ApiResponse::ok("Username updated");
    body.message_key = Some("api.security.usernameUpdated".to_string());
    Ok(body.into_response())
}

fn validate_safe_entry_for_api(value: &str) -> ApiResult<()> {
    validate_safe_entry_value(value).map_err(|err| {
        let message = match err {
            SafeEntryValidationError::InvalidFormat => {
                "safe entry must be 8-32 ASCII letters or digits"
            }
            SafeEntryValidationError::ReservedPrefix => "safe entry uses a reserved path prefix",
        };
        ApiError::validation(message).with_message_key("api.security.safeEntryInvalid")
    })
}

fn validate_username(value: &str) -> ApiResult<()> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .enumerate()
            .all(|(idx, ch)| ch.is_ascii_alphanumeric() || ch == '_' || (idx > 0 && ch == '-'));
    if !valid {
        return Err(ApiError::validation(
            "username must be 1-64 ASCII letters, digits, underscore, or hyphen",
        )
        .with_message_key("api.security.usernameInvalid"));
    }
    Ok(())
}
