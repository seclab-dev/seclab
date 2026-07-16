-- 平台事件日志与通知历史。
CREATE TABLE IF NOT EXISTS platform_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NULL REFERENCES users(id) ON DELETE SET NULL,
    username TEXT NOT NULL,
    module TEXT NOT NULL,
    event TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    status TEXT NOT NULL,
    level TEXT NOT NULL DEFAULT 'INFO' CHECK (level IN ('INFO', 'WARNING', 'ERROR')),
    client_ip TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'seclab',
    request_path TEXT NOT NULL DEFAULT '',
    method TEXT NOT NULL DEFAULT '',
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_platform_logs_timestamp ON platform_logs (timestamp);
CREATE INDEX IF NOT EXISTS idx_platform_logs_module_event ON platform_logs (module, event);
CREATE INDEX IF NOT EXISTS idx_platform_logs_user_id ON platform_logs (user_id);
CREATE INDEX IF NOT EXISTS idx_platform_logs_trace_id ON platform_logs (trace_id);
CREATE INDEX IF NOT EXISTS idx_platform_logs_level_timestamp ON platform_logs (level, timestamp DESC);

CREATE TABLE IF NOT EXISTS notification_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    level TEXT NOT NULL,
    title TEXT, -- 通知标题，用于未来区分标题与正文。
    message TEXT NOT NULL,
    message_key TEXT, -- 国际化消息键，套件或主控可用该字段延迟渲染文案。
    payload_json TEXT NOT NULL DEFAULT '{}', -- 通知渲染与跳转所需的结构化参数。
    source TEXT NOT NULL DEFAULT 'seclab', -- 通知来源，例如主控、Agent 或套件。
    read_at INTEGER, -- 用户读取通知的时间戳，空值表示未读。
    metadata TEXT NOT NULL DEFAULT '{}', -- 预留扩展字段，避免为展示上下文频繁新增列。
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_notification_history_created_at ON notification_history (created_at DESC);
