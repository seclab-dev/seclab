-- Docker 操作日志。Alpha 阶段不迁移旧 platform_logs 数据。
CREATE TABLE IF NOT EXISTS docker_activity_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('user', 'system')),
    actor_name TEXT NOT NULL,
    client_ip TEXT NOT NULL DEFAULT '',
    level TEXT NOT NULL CHECK (level IN ('info', 'warning', 'error')),
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
    event_code TEXT NOT NULL,
    target_kind TEXT NOT NULL DEFAULT '',
    target_id TEXT NOT NULL DEFAULT '',
    message_params TEXT NOT NULL DEFAULT '{}',
    error_message TEXT,
    trace_id TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_docker_activity_logs_time
    ON docker_activity_logs (occurred_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_docker_activity_logs_level_time
    ON docker_activity_logs (level, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_docker_activity_logs_actor_time
    ON docker_activity_logs (actor_kind, actor_name, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_docker_activity_logs_target
    ON docker_activity_logs (target_kind, target_id);
