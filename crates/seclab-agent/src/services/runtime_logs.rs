//! 运行日志文件查询服务：读取当前 Agent 本机的 `tracing` 轮转日志。

use crate::types::{ApiError, ApiResult};
use seclab_contracts::runtime_logs::{
    RuntimeLogFile, RuntimeLogLine, RuntimeLogQuery, RuntimeLogQueryResult,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::io::AsyncSeekExt;

const DEFAULT_LIMIT: u32 = 200;
const MAX_LIMIT: u32 = 1000;
const DEFAULT_PRODUCTION_HOME: &str = "/opt/seclab";

/// 返回 Agent 运行日志根目录。开发环境可通过 `SECLAB_LOG_DIR` 覆盖。
pub fn runtime_log_root() -> PathBuf {
    std::env::var_os("SECLAB_LOG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("SECLAB_DATA_DIR").map(|value| PathBuf::from(value).join("logs"))
        })
        .unwrap_or_else(default_log_root)
}

/// 返回 Agent 运行日志目录。
pub fn service_log_dir(service: &str) -> ApiResult<PathBuf> {
    match service {
        "agent" => Ok(runtime_log_root().join(service)),
        _ => Err(ApiError::BadRequest(
            "unsupported runtime log service".to_string(),
        )),
    }
}

/// 查询当前 Agent 本机运行日志文件列表。
pub async fn list_runtime_log_files() -> ApiResult<Vec<RuntimeLogFile>> {
    let mut files = Vec::new();
    let dir = service_log_dir("agent")?;
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(err) => return Err(ApiError::Io(err)),
    };

    while let Some(entry) = entries.next_entry().await.map_err(ApiError::Io)? {
        let metadata = entry.metadata().await.map_err(ApiError::Io)?;
        if !metadata.is_file() {
            continue;
        }
        let Some(file_name) = entry.file_name().to_str().map(ToString::to_string) else {
            continue;
        };

        files.push(RuntimeLogFile {
            service: "agent".to_string(),
            node_id: None,
            node_name: None,
            file_name,
            size_bytes: metadata.len(),
            modified_at: metadata.modified().ok().and_then(format_system_time),
        });
    }

    files.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| left.file_name.cmp(&right.file_name))
    });
    Ok(files)
}

/// 查询当前 Agent 指定运行日志文件片段。
pub async fn query_runtime_logs(query: RuntimeLogQuery) -> ApiResult<RuntimeLogQueryResult> {
    if query.service != "agent" {
        return Err(ApiError::BadRequest(
            "Agent only supports querying agent logs".to_string(),
        ));
    }

    let limit = query
        .limit
        .clamp(1, MAX_LIMIT)
        .max(DEFAULT_LIMIT.min(query.limit.max(1)));
    let path = resolve_log_file(&query.file_name)?;
    let mut file = tokio::fs::File::open(path).await.map_err(ApiError::Io)?;
    let file_len = file.metadata().await.map_err(ApiError::Io)?.len();

    let mut cursor = query.cursor.unwrap_or(file_len);
    let mut lines = Vec::new();
    let mut has_more = false;

    const BUFFER_SIZE: usize = 64 * 1024;
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut leftover = Vec::new();

    while cursor > 0 && lines.len() < limit as usize {
        let read_len = (cursor as usize).min(BUFFER_SIZE);
        let start_offset = cursor - read_len as u64;

        file.seek(std::io::SeekFrom::Start(start_offset))
            .await
            .map_err(ApiError::Io)?;

        use tokio::io::AsyncReadExt;
        file.read_exact(&mut buffer[..read_len])
            .await
            .map_err(ApiError::Io)?;

        cursor = start_offset;

        let mut chunk_data = Vec::new();
        chunk_data.extend_from_slice(&buffer[..read_len]);
        chunk_data.extend_from_slice(&leftover);
        leftover.clear();

        let parts: Vec<&[u8]> = chunk_data.split(|&b| b == b'\n').collect();
        let mut parts_with_offset = Vec::new();
        let mut current_part_offset = start_offset;

        for part in &parts {
            parts_with_offset.push((current_part_offset, *part));
            current_part_offset += part.len() as u64 + 1;
        }

        if cursor > 0 && !parts_with_offset.is_empty() {
            leftover = parts_with_offset.remove(0).1.to_vec();
        }

        for (line_offset, part) in parts_with_offset.into_iter().rev() {
            let raw_str = String::from_utf8_lossy(part);
            let raw_trimmed = raw_str.trim_end_matches(['\r', '\n']).to_string();
            if raw_trimmed.is_empty() {
                continue;
            }

            let parsed = parse_runtime_log_line(line_offset, &raw_trimmed);
            if runtime_line_matches(&parsed, &raw_trimmed, &query) {
                lines.push(parsed);
                if lines.len() >= limit as usize {
                    cursor = line_offset;
                    has_more = cursor > 0;
                    break;
                }
            }
        }
    }

    if cursor == 0 && !leftover.is_empty() && lines.len() < limit as usize {
        let raw_str = String::from_utf8_lossy(&leftover);
        let raw_trimmed = raw_str.trim_end_matches(['\r', '\n']).to_string();
        if !raw_trimmed.is_empty() {
            let parsed = parse_runtime_log_line(0, &raw_trimmed);
            if runtime_line_matches(&parsed, &raw_trimmed, &query) {
                lines.push(parsed);
            }
        }
    }

    Ok(RuntimeLogQueryResult {
        lines,
        next_cursor: Some(cursor),
        has_more,
    })
}

fn resolve_log_file(file_name: &str) -> ApiResult<PathBuf> {
    let name = Path::new(file_name);
    if name.components().count() != 1 {
        return Err(ApiError::BadRequest(
            "invalid runtime log file name".to_string(),
        ));
    }
    Ok(service_log_dir("agent")?.join(name))
}

fn parse_runtime_log_line(offset: u64, raw: &str) -> RuntimeLogLine {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return RuntimeLogLine {
            offset,
            timestamp: None,
            level: None,
            target: None,
            source: None,
            message: raw.to_string(),
            parse_error: true,
        };
    };

    RuntimeLogLine {
        offset,
        timestamp: json_string(&value, "timestamp"),
        level: json_string(&value, "level"),
        target: json_string(&value, "target"),
        source: extract_source(&value),
        message: extract_message(&value).unwrap_or_default(),
        parse_error: false,
    }
}

fn runtime_line_matches(line: &RuntimeLogLine, raw: &str, query: &RuntimeLogQuery) -> bool {
    if let Some(level) = query.level.as_deref()
        && !level.trim().is_empty()
        && line.level.as_deref() != Some(level)
    {
        return false;
    }
    if let Some(target) = query.target.as_deref()
        && !target.trim().is_empty()
        && !line.target.as_deref().unwrap_or_default().contains(target)
    {
        return false;
    }
    if let Some(keyword) = query.keyword.as_deref() {
        let keyword = keyword.trim();
        if !keyword.is_empty() && !raw.contains(keyword) && !line.message.contains(keyword) {
            return false;
        }
    }
    true
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn extract_message(value: &Value) -> Option<String> {
    json_string_in_event(value, "message")
}

fn extract_source(value: &Value) -> Option<String> {
    json_string_in_event(value, "client_ip").and_then(|item| normalize_client_ip(&item))
}

fn json_string_in_event(value: &Value, key: &str) -> Option<String> {
    json_string(value, key)
        .or_else(|| {
            value
                .get("fields")
                .and_then(|fields| json_string(fields, key))
        })
        .or_else(|| value.get("span").and_then(|span| json_string(span, key)))
        .or_else(|| {
            value
                .get("spans")
                .and_then(Value::as_array)
                .and_then(|spans| spans.iter().rev().find_map(|span| json_string(span, key)))
        })
}

fn normalize_client_ip(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("[::ffff:")
        && let Some((ip, _)) = rest.split_once("]:")
    {
        return Some(ip.to_string());
    }

    if let Some(rest) = trimmed.strip_prefix('[')
        && let Some((ip, _)) = rest.split_once("]:")
    {
        return Some(ip.to_string());
    }

    trimmed
        .rsplit_once(':')
        .and_then(|(ip, port)| port.parse::<u16>().ok().map(|_| ip.to_string()))
        .or_else(|| Some(trimmed.to_string()))
}

fn format_system_time(value: SystemTime) -> Option<String> {
    let datetime: chrono::DateTime<chrono::Utc> = value.into();
    Some(datetime.to_rfc3339())
}

fn workspace_dev_dir() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for ancestor in cwd.ancestors() {
        if ancestor.join("crates").is_dir() && ancestor.join("frontend").is_dir() {
            return ancestor.join(".seclab");
        }
    }
    PathBuf::from(".seclab")
}

fn default_log_root() -> PathBuf {
    if cfg!(debug_assertions) {
        workspace_dev_dir().join("logs")
    } else {
        production_home().join("logs")
    }
}

fn production_home() -> PathBuf {
    std::env::var_os("SECLAB_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PRODUCTION_HOME))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_query(keyword: Option<&str>) -> RuntimeLogQuery {
        RuntimeLogQuery {
            service: "agent".to_string(),
            node_id: None,
            file_name: "agent.log".to_string(),
            level: None,
            target: None,
            keyword: keyword.map(ToString::to_string),
            cursor: None,
            limit: 100,
        }
    }

    #[test]
    fn parse_runtime_log_line_extracts_fields_source() {
        let raw = r#"{"timestamp":"2026-06-18T12:00:00+08:00","level":"INFO","target":"seclab_agent::api","fields":{"message":"request completed","username":"admin","client_ip":"127.0.0.1","hidden":"needle"}}"#;

        let line = parse_runtime_log_line(42, raw);

        assert_eq!(line.offset, 42);
        assert_eq!(line.message, "request completed");
        assert_eq!(line.source.as_deref(), Some("127.0.0.1"));
        assert!(!line.parse_error);
        assert!(runtime_line_matches(
            &line,
            raw,
            &test_query(Some("needle"))
        ));
    }

    #[test]
    fn parse_runtime_log_line_extracts_top_level_client_ip_source() {
        let raw = r#"{"timestamp":"2026-06-18T12:00:00+08:00","level":"INFO","target":"seclab_agent::node","message":"node updated","client_ip":"10.0.0.12"}"#;

        let line = parse_runtime_log_line(0, raw);

        assert_eq!(line.message, "node updated");
        assert_eq!(line.source.as_deref(), Some("10.0.0.12"));
    }

    #[test]
    fn parse_runtime_log_line_extracts_span_client_ip_source() {
        let raw = r#"{"timestamp":"2026-06-18T11:04:06.508+08:00","level":"INFO","fields":{"message":"finished POST /api/v1/platform/runtime-logs/query with 200 in 4.729937ms"},"target":"seclab_api::logging","span":{"client_ip":"[::ffff:10.121.111.125]:51440","method":"POST","path":"\"/api/v1/platform/runtime-logs/query\"","name":"seclab"},"spans":[{"client_ip":"[::ffff:10.121.111.125]:51440","method":"POST","path":"\"/api/v1/platform/runtime-logs/query\"","name":"seclab"}]}"#;

        let line = parse_runtime_log_line(0, raw);

        assert_eq!(
            line.message,
            "finished POST /api/v1/platform/runtime-logs/query with 200 in 4.729937ms"
        );
        assert_eq!(line.source.as_deref(), Some("10.121.111.125"));
    }

    #[test]
    fn parse_runtime_log_line_does_not_use_node_name_as_source() {
        let raw = r#"{"timestamp":"2026-06-18T12:00:00+08:00","level":"INFO","target":"seclab_agent::node","message":"node updated","nodeName":"edge-01"}"#;

        let line = parse_runtime_log_line(0, raw);

        assert_eq!(line.message, "node updated");
        assert!(line.source.is_none());
    }

    #[test]
    fn parse_runtime_log_line_keeps_invalid_line_as_message() {
        let raw = "not-json";

        let line = parse_runtime_log_line(0, raw);

        assert_eq!(line.message, raw);
        assert!(line.source.is_none());
        assert!(line.parse_error);
    }
}
