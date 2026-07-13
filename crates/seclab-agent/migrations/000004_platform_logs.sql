-- Agent 本地平台事件日志。
CREATE TABLE IF NOT EXISTS platform_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NULL,
    username TEXT NOT NULL,
    module TEXT NOT NULL,
    event TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    status TEXT NOT NULL,
    client_ip TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'agent',
    request_path TEXT NOT NULL DEFAULT '',
    method TEXT NOT NULL DEFAULT '',
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_agent_logs_timestamp ON platform_logs (timestamp);
CREATE INDEX IF NOT EXISTS idx_agent_logs_module_event ON platform_logs (module, event);
CREATE INDEX IF NOT EXISTS idx_agent_logs_user_id ON platform_logs (user_id);
CREATE INDEX IF NOT EXISTS idx_agent_logs_trace_id ON platform_logs (trace_id);
