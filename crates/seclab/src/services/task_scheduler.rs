//! 计划任务调度服务：分钟级 cron 表达式解析与校验。

use crate::types::{ApiError, ApiResult};
use chrono::{TimeZone, Utc};
use cron::Schedule;
use std::str::FromStr;

pub fn validate_cron_expr(cron_expr: &str) -> ApiResult<()> {
    let minute_expr = normalize_minute_cron_expr(cron_expr)?;
    let schedule_expr = format!("0 {minute_expr}");
    Schedule::from_str(&schedule_expr)
        .map(|_| ())
        .map_err(|err| ApiError::BadRequest(format!("invalid cron expression: {err}")))
}

pub fn compute_next_run_at(cron_expr: &str, from_ts: i64) -> ApiResult<Option<i64>> {
    let minute_expr = normalize_minute_cron_expr(cron_expr)?;
    let schedule_expr = format!("0 {minute_expr}");
    let schedule = Schedule::from_str(&schedule_expr)
        .map_err(|err| ApiError::BadRequest(format!("invalid cron expression: {err}")))?;
    let base = Utc
        .timestamp_opt(from_ts, 0)
        .single()
        .unwrap_or_else(Utc::now);
    Ok(schedule.after(&base).next().map(|next| next.timestamp()))
}

fn normalize_minute_cron_expr(cron_expr: &str) -> ApiResult<String> {
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();
    match parts.as_slice() {
        [minute, hour, day, month, weekday] => {
            Ok(format!("{minute} {hour} {day} {month} {weekday}"))
        }
        [second, minute, hour, day, month, weekday] if *second == "0" => {
            Ok(format!("{minute} {hour} {day} {month} {weekday}"))
        }
        [_, ..] if parts.len() == 6 => Err(ApiError::BadRequest(
            "only minute-level cron expressions are supported; seconds-level triggers are not supported".to_string(),
        )),
        _ => Err(ApiError::BadRequest(
            "cron expression must contain 5 fields: minute hour day month weekday".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_helpers() {
        assert!(validate_cron_expr("*/5 * * * *").is_ok());
        assert!(validate_cron_expr("0 0 1 1 *").is_ok());
        assert!(validate_cron_expr("invalid").is_err());

        let next = compute_next_run_at("*/5 * * * *", 1782310745).unwrap();
        assert_eq!(next, Some(1782310800));
    }
}
