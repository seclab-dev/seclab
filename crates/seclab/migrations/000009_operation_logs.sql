-- 操作审计事件与个人通知中心。
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

CREATE TABLE user_notifications (
    notification_id TEXT PRIMARY KEY,
    recipient_user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    operation_event_id TEXT NOT NULL REFERENCES operation_logs(event_id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    code TEXT NOT NULL,
    category TEXT NOT NULL CHECK (category IN ('task', 'security', 'system')),
    attention_level TEXT NOT NULL CHECK (attention_level IN ('info', 'warning', 'critical')),
    outcome TEXT CHECK (outcome IS NULL OR outcome IN ('success', 'failure', 'partial', 'canceled', 'timedOut')),
    source_module TEXT NOT NULL,
    source_node_id TEXT,
    source_node_name TEXT,
    subject_kind TEXT,
    subject_id TEXT,
    subject_display_name TEXT,
    task_id TEXT,
    parameters_json TEXT NOT NULL DEFAULT '{}',
    error_code TEXT,
    error_summary TEXT,
    trace_id TEXT NOT NULL,
    read_at TEXT,
    archived_at TEXT,
    state_changed_at TEXT NOT NULL,
    UNIQUE (recipient_user_id, operation_event_id)
);

CREATE INDEX idx_user_notifications_recipient_archive_created
    ON user_notifications (recipient_user_id, archived_at, created_at DESC, notification_id DESC);
CREATE INDEX idx_user_notifications_recipient_unread
    ON user_notifications (recipient_user_id, created_at DESC, notification_id DESC)
    WHERE read_at IS NULL AND archived_at IS NULL;
