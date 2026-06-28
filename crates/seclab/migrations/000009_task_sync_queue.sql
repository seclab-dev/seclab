-- 计划任务同步变更队列。
CREATE TABLE task_sync_ops (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    agent_id TEXT NOT NULL,
    op_type TEXT NOT NULL,
    revision INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    error_message TEXT,
    attempts INTEGER NOT NULL DEFAULT 0, -- 同步尝试次数，用于重试和故障诊断。
    last_attempt_at TEXT, -- 最近一次同步尝试时间。
    completed_at TEXT, -- 同步完成时间。
    metadata TEXT NOT NULL DEFAULT '{}', -- 同步上下文和扩展调度信息。
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_task_sync_ops_status ON task_sync_ops (status);
