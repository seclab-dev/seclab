-- Agent 操作审计持久 outbox；最终结果由 Master 统一查询。
CREATE TABLE operation_event_outbox (
    event_id TEXT PRIMARY KEY,
    event_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,
    delivered_at TEXT
);

CREATE INDEX idx_operation_event_outbox_pending
    ON operation_event_outbox (next_attempt_at, created_at, event_id)
    WHERE delivered_at IS NULL;
