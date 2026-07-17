-- 操作审计事件与通知历史。
CREATE TABLE operation_logs (
    event_id TEXT PRIMARY KEY,
    occurred_at TEXT NOT NULL,
    module TEXT NOT NULL,
    event_code TEXT NOT NULL,
    actor_kind TEXT NOT NULL,
    actor_user_id INTEGER NULL REFERENCES users(id) ON DELETE SET NULL,
    actor_display_name TEXT NOT NULL,
    origin_kind TEXT NOT NULL,
    origin_node_id TEXT,
    origin_node_name TEXT,
    target_kind TEXT,
    target_id TEXT,
    target_display_name TEXT,
    target_ownership TEXT,
    outcome TEXT NOT NULL,
    impact TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    task_id TEXT,
    client_ip TEXT NOT NULL,
    request_method TEXT,
    route_template TEXT,
    parameters_json TEXT NOT NULL DEFAULT '{}',
    error_code TEXT,
    error_summary TEXT
);

CREATE INDEX idx_operation_logs_occurred_event
    ON operation_logs (occurred_at DESC, event_id DESC);
CREATE INDEX idx_operation_logs_module_occurred
    ON operation_logs (module, occurred_at DESC, event_id DESC);
CREATE INDEX idx_operation_logs_actor_occurred
    ON operation_logs (actor_user_id, occurred_at DESC, event_id DESC);
CREATE INDEX idx_operation_logs_origin_occurred
    ON operation_logs (origin_node_id, occurred_at DESC, event_id DESC);
CREATE INDEX idx_operation_logs_trace_id ON operation_logs (trace_id);

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
