//! 认证 API：单用户管理员登录、服务端会话校验与退出。

use crate::models::auth_sessions::{
    AuthSessionRecord, CreateAuthSessionInput, create_auth_session, get_auth_session_by_hash,
    hash_session_token, is_session_active, revoke_auth_session_by_token, touch_auth_session,
};
use crate::models::logging::LogModule;
use crate::models::user::User;
use crate::services::logging::{self, PlatformLogEntry};
use crate::state::AppState;
use crate::types::{ApiError, ApiResponse, ApiResult};
use axum::{
    Json, Router,
    extract::{FromRef, FromRequestParts, Request, State, connect_info::ConnectInfo},
    http::{HeaderMap, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie as AxumCookie, SameSite};
use bcrypt::verify;
use seclab_contracts::auth::AuthBody;
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;

const SESSION_COOKIE: &str = "seclab_session";
const SESSION_COOKIE_MAX_AGE_SECONDS: i64 = 12 * 60 * 60;

/// 已通过服务端会话校验的管理员身份。
#[derive(Debug, Clone)]
pub struct AuthenticatedAdmin {
    pub id: i64,
    pub username: String,
    pub session: AuthSessionRecord,
}

/// 登录请求载荷。
#[derive(Debug, Deserialize)]
pub struct AuthPayload {
    pub username: String,
    pub password: String,
}

/// 处理管理员登录请求，成功后创建可并存的服务端会话。
pub async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<AuthPayload>,
) -> ApiResult<Response> {
    let client_ip = conn.ip();
    let pool = &state.metadata_db;
    let trace_id = logging::resolve_trace_id(&headers);

    if payload.username.is_empty() || payload.password.is_empty() {
        logging::platform_log_failure(&payload.username, "user_login", client_ip)
            .module(LogModule::Auth)
            .target_type("user")
            .target_id(&payload.username)
            .trace_id(&trace_id)
            .source("seclab_api")
            .request("POST", "/api/v1/auth/login")
            .metadata(json!({
                "reason": "MissingCredentials",
                "auth_stage": "input_validate",
                "attempted_user": &payload.username,
                "message_key": "platformLog.auth.login.missingCredentials"
            }))
            .finish(pool);
        return Err(ApiError::MissingCredentials);
    }

    let user_record = sqlx::query_as::<_, User>(
        r#"
        SELECT id, username, password_hash, status, password_changed_at, created_at, updated_at
        FROM users
        WHERE username = ?1
        "#,
    )
    .bind(&payload.username)
    .fetch_one(pool)
    .await
    .map_err(|err| {
        let log_reason = match err {
            sqlx::Error::RowNotFound => "UserNotFound",
            _ => "DatabaseError",
        };

        logging::platform_log_failure(&payload.username, "user_login", client_ip)
            .module(LogModule::Auth)
            .target_type("user")
            .target_id(&payload.username)
            .trace_id(&trace_id)
            .source("seclab_api")
            .request("POST", "/api/v1/auth/login")
            .metadata(json!({
                "reason": log_reason,
                "auth_stage": "user_lookup",
                "attempted_user": &payload.username,
                "message_key": "platformLog.auth.login.userLookupFailed"
            }))
            .finish(pool);

        match err {
            sqlx::Error::RowNotFound => ApiError::WrongCredentials,
            _ => err.into(),
        }
    })?;

    if !user_record.is_active() {
        logging::platform_log_failure(&payload.username, "user_login", client_ip)
            .module(LogModule::Auth)
            .target_type("user")
            .target_id(&user_record.id.to_string())
            .user_id(user_record.id)
            .trace_id(&trace_id)
            .source("seclab_api")
            .request("POST", "/api/v1/auth/login")
            .metadata(json!({
                "reason": "AdminDisabled",
                "auth_stage": "user_status",
                "attempted_user": &payload.username,
                "message_key": "platformLog.auth.login.userLookupFailed"
            }))
            .finish(pool);
        return Err(ApiError::WrongCredentials);
    }

    let valid_password = verify(&payload.password, &user_record.password_hash).map_err(|err| {
        logging::platform_log_failure(&payload.username, "user_login", client_ip)
            .module(LogModule::Auth)
            .target_type("user")
            .target_id(&user_record.id.to_string())
            .user_id(user_record.id)
            .trace_id(&trace_id)
            .source("seclab_api")
            .request("POST", "/api/v1/auth/login")
            .metadata(json!({
                "reason": "BcryptInternalError",
                "auth_stage": "password_verify",
                "error_detail": err.to_string(),
                "attempted_user": &payload.username,
                "target_user_id": user_record.id,
                "target_username": &user_record.username,
                "message_key": "platformLog.auth.login.passwordVerifyError"
            }))
            .finish(pool);

        tracing::error!(
            "Bcrypt verification internal error for user {}: {:?}",
            user_record.username,
            err
        );
        ApiError::WrongCredentials
    })?;

    if !valid_password {
        logging::platform_log_failure(&payload.username, "user_login", client_ip)
            .module(LogModule::Auth)
            .target_type("user")
            .target_id(&user_record.id.to_string())
            .user_id(user_record.id)
            .trace_id(&trace_id)
            .source("seclab_api")
            .request("POST", "/api/v1/auth/login")
            .metadata(json!({
                "reason": "InvalidPassword",
                "auth_stage": "password_verify",
                "attempted_user": &payload.username,
                "target_user_id": user_record.id,
                "target_username": &user_record.username,
                "message_key": "platformLog.auth.login.invalidPassword"
            }))
            .finish(pool);
        return Err(ApiError::WrongCredentials);
    }

    let created = create_auth_session(
        pool,
        CreateAuthSessionInput {
            user_id: user_record.id,
            client_ip: Some(client_ip.to_string()),
            user_agent: headers
                .get(header::USER_AGENT)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string()),
        },
    )
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    logging::platform_log_success(&payload.username, "user_login", client_ip)
        .module(LogModule::Auth)
        .target_type("user")
        .target_id(&user_record.id.to_string())
        .user_id(user_record.id)
        .trace_id(&trace_id)
        .source("seclab_api")
        .request("POST", "/api/v1/auth/login")
        .metadata(json!({
            "message_key": "platformLog.auth.login.success",
            "session_id": created.record.id
        }))
        .finish(pool);

    let mut resp = ApiResponse::success_with_raw(
        "Login succeeded",
        Some(AuthBody::new(
            user_record.id,
            user_record.username,
            created.record.id,
            created.record.expires_at,
        )),
    )
    .into_response();

    set_auth_cookie(&mut resp, &created.token);
    Ok(resp)
}

/// 返回当前有效管理员会话。
pub async fn me(admin: AuthenticatedAdmin) -> ApiResult<impl IntoResponse> {
    Ok(ApiResponse::success_with_raw(
        "Current session loaded",
        Some(AuthBody::new(
            admin.id,
            admin.username,
            admin.session.id,
            admin.session.expires_at,
        )),
    )
    .into_response())
}

/// 撤销当前认证会话并清理 Cookie。
pub async fn logout(
    State(state): State<Arc<AppState>>,
    ConnectInfo(conn): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let trace_id = logging::resolve_trace_id(&headers);
    let token = extract_cookie_from_headers(&headers, SESSION_COOKIE);

    if let Some(token) = token {
        let token_hash = hash_session_token(&token);
        let session = get_auth_session_by_hash(&state.metadata_db, &token_hash).await?;
        if let Some(session) = session {
            let user = get_active_admin_by_id(&state.metadata_db, session.user_id)
                .await
                .ok()
                .flatten();
            let username = user
                .as_ref()
                .map(|value| value.username.as_str())
                .unwrap_or("anonymous");
            let mut platform_log = PlatformLogEntry::new(username, "user_logout", conn.ip())
                .module(LogModule::Auth)
                .target_type("user")
                .target_id(&session.user_id.to_string())
                .trace_id(&trace_id)
                .source("seclab_api")
                .request("POST", "/api/v1/auth/logout")
                .set_success()
                .metadata(json!({
                    "message_key": "platformLog.auth.logout.success",
                    "session_id": session.id
                }));
            if let Some(user) = user {
                platform_log = platform_log.user_id(user.id);
            }
            platform_log.finish(&state.metadata_db);

            revoke_auth_session_by_token(&state.metadata_db, &token, "logout").await?;
        } else {
            log_logout_without_session(&state, conn.ip(), &trace_id);
        }
    } else {
        log_logout_without_session(&state, conn.ip(), &trace_id);
    }

    let mut resp = ApiResponse::success_with_raw("Logout succeeded", ()).into_response();
    clear_auth_cookie(&mut resp);
    Ok(resp)
}

/// 创建认证相关路由。
pub fn auth_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/login", post(login))
        .route("/me", get(me))
        .route("/logout", post(logout))
}

impl<S> FromRequestParts<S> for AuthenticatedAdmin
where
    Arc<AppState>: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        if let Some(admin) = parts.extensions.get::<AuthenticatedAdmin>() {
            return Ok(admin.clone());
        }
        let state = Arc::<AppState>::from_ref(state);
        authenticate_parts(parts, &state).await
    }
}

/// 认证中间件，用于在进入受保护路由前统一阻断无效会话。
pub async fn auth_layer(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> ApiResult<Response> {
    let path = req.uri().path();
    // 升级期间，如果是普通业务请求则直接拦截，但需要放行 upgrades 相关的状态轮询接口
    if !is_upgrade_api_path(path) && is_system_upgrading(&state.metadata_db).await {
        return Err(ApiError::new(
            axum::http::StatusCode::LOCKED,
            seclab_contracts::api::ErrorCode::AuthForbidden,
            "System is currently upgrading, please wait.",
        )
        .with_message_key("api.upgrades.systemIsUpgrading"));
    }

    let (mut parts, body) = req.into_parts();
    let admin = authenticate_parts(&mut parts, &state).await?;
    parts.extensions.insert(admin);
    let req = Request::from_parts(parts, body);
    Ok(next.run(req).await)
}

fn is_upgrade_api_path(path: &str) -> bool {
    path.starts_with("/api/v1/upgrades/") || path.starts_with("/upgrades/")
}

async fn is_system_upgrading(pool: &sqlx::SqlitePool) -> bool {
    let result: Result<i64, _> =
        sqlx::query_scalar("SELECT COUNT(*) FROM upgrade_plans WHERE status = 'running'")
            .fetch_one(pool)
            .await;
    result.unwrap_or(0) > 0
}

async fn authenticate_parts(
    parts: &mut Parts,
    state: &Arc<AppState>,
) -> Result<AuthenticatedAdmin, ApiError> {
    let token = extract_cookie(parts, SESSION_COOKIE)?;
    let token_hash = hash_session_token(&token);
    let session = get_auth_session_by_hash(&state.metadata_db, &token_hash)
        .await?
        .ok_or(ApiError::InvalidToken)?;
    if !is_session_active(&session) {
        return Err(ApiError::InvalidToken);
    }

    let user = get_active_admin_by_id(&state.metadata_db, session.user_id)
        .await?
        .ok_or(ApiError::InvalidToken)?;
    touch_auth_session(&state.metadata_db, &session.id).await?;

    Ok(AuthenticatedAdmin {
        id: user.id,
        username: user.username,
        session,
    })
}

async fn get_active_admin_by_id(
    pool: &sqlx::SqlitePool,
    user_id: i64,
) -> sqlx::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        r#"
        SELECT id, username, password_hash, status, password_changed_at, created_at, updated_at
        FROM users
        WHERE id = ?1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(user.filter(User::is_active))
}

fn log_logout_without_session(state: &AppState, client_ip: std::net::IpAddr, trace_id: &str) {
    PlatformLogEntry::new("anonymous", "user_logout", client_ip)
        .module(LogModule::Auth)
        .target_type("user")
        .target_id("anonymous")
        .trace_id(trace_id)
        .source("seclab_api")
        .request("POST", "/api/v1/auth/logout")
        .set_success()
        .metadata(json!({ "message_key": "platformLog.auth.logout.noSession" }))
        .finish(&state.metadata_db);
}

fn build_cookie(name: &str, value: &str, max_age_seconds: i64, path: &str) -> AxumCookie<'static> {
    let mut cookie = AxumCookie::build((name.to_string(), value.to_string()))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path(path.to_string())
        .max_age(cookie::time::Duration::seconds(max_age_seconds))
        .build();
    if crate::config::get().cookie_secure {
        cookie.set_secure(true);
    }
    cookie
}

fn set_auth_cookie(resp: &mut Response, token: &str) {
    let cookie = build_cookie(SESSION_COOKIE, token, SESSION_COOKIE_MAX_AGE_SECONDS, "/");
    resp.headers_mut()
        .append(header::SET_COOKIE, cookie.to_string().parse().unwrap());
}

fn clear_auth_cookie(resp: &mut Response) {
    let cookie = build_cookie(SESSION_COOKIE, "", 0, "/");
    resp.headers_mut()
        .append(header::SET_COOKIE, cookie.to_string().parse().unwrap());
}

fn extract_cookie(parts: &Parts, cookie_name: &str) -> Result<String, ApiError> {
    extract_cookie_from_headers(&parts.headers, cookie_name).ok_or(ApiError::InvalidToken)
}

fn extract_cookie_from_headers(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let value = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in value.split(';') {
        let trimmed = part.trim();
        if let Some((name, val)) = trimmed.split_once('=')
            && name == cookie_name
        {
            return Some(val.to_string());
        }
    }
    None
}
