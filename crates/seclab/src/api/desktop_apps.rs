//! 桌面应用 API：桌面快捷方式与排序相关接口。

use crate::models::apps::list_apps;
use crate::models::desktop_apps::{
    DesktopAppItem, DesktopAppItemPayload, DesktopAppsPayload, fetch_desktop_apps,
    replace_desktop_apps,
};
use crate::state::AppState;
use crate::types::{ApiResponse, ApiResult};
use axum::{Json, Router, extract::State, response::IntoResponse, routing::get};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// 封装桌面应用列表与是否已配置过的标记。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAppsResponse {
    pub apps: Vec<DesktopAppItem>,
    pub has_record: bool,
}

/// 构建桌面应用相关的路由集合。
pub fn desktop_apps_router() -> Router<Arc<AppState>> {
    Router::new().route("/shortcuts", get(list).put(update))
}

/// 返回当前桌面应用排序与可见性配置。
pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<impl IntoResponse> {
    let app_ids = fetch_desktop_apps(&state.metadata_db).await?;
    let response = DesktopAppsResponse {
        apps: app_ids.clone().unwrap_or_default(),
        has_record: app_ids.is_some(),
    };
    Ok(ApiResponse::success_with_raw("Desktop applications loaded", response).into_response())
}

/// 保存桌面应用排序与可见性，并补全缺失项的默认值。
pub async fn update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DesktopAppsPayload>,
) -> ApiResult<impl IntoResponse> {
    let mut seen = HashSet::new();
    let payload_items: Vec<DesktopAppItemPayload> = payload
        .apps
        .into_iter()
        .filter(|item| seen.insert(item.app_id.clone()))
        .collect();
    let catalog = list_apps(&state.metadata_db, None).await?;
    let payload_map: HashMap<String, DesktopAppItemPayload> = payload_items
        .into_iter()
        .map(|item| (item.app_id.clone(), item))
        .collect();
    let catalog_ids: HashSet<String> = catalog.iter().map(|app| app.app_id.clone()).collect();
    let mut items: Vec<DesktopAppItemPayload> = catalog
        .into_iter()
        .map(|app| {
            payload_map
                .get(&app.app_id)
                .cloned()
                .unwrap_or(DesktopAppItemPayload {
                    app_id: app.app_id,
                    sort_order: app.sort_order,
                    visible: false,
                    x: None,
                    y: None,
                    suite_visible_before_disable: None,
                })
        })
        .collect();
    if let Some(existing_items) = fetch_desktop_apps(&state.metadata_db).await? {
        items.extend(
            existing_items
                .into_iter()
                .filter(|item| {
                    item.app_id.starts_with("suite:")
                        && !catalog_ids.contains(&item.app_id)
                        && !payload_map.contains_key(&item.app_id)
                })
                .map(|item| DesktopAppItemPayload {
                    app_id: item.app_id,
                    sort_order: item.sort_order,
                    visible: item.visible,
                    x: item.x,
                    y: item.y,
                    suite_visible_before_disable: item.suite_visible_before_disable,
                }),
        );
    }
    replace_desktop_apps(&state.metadata_db, &items).await?;
    Ok(ApiResponse::ok("Desktop applications saved").into_response())
}
