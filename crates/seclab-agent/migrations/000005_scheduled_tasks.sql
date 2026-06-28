-- Agent 本地计划任务。
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    controller_task_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    cron_expr TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    timeout_secs INTEGER NOT NULL DEFAULT 60,
    no_overlap INTEGER NOT NULL DEFAULT 1,
    last_run_at INTEGER,
    next_run_at INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (controller_task_id)
);

CREATE INDEX IF NOT EXISTS idx_agent_scheduled_tasks_enabled_next_run
    ON scheduled_tasks(enabled, next_run_at);

CREATE TRIGGER IF NOT EXISTS set_agent_scheduled_tasks_updated_at
AFTER UPDATE ON scheduled_tasks FOR EACH ROW
BEGIN
    UPDATE scheduled_tasks
       SET updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE controller_task_id = OLD.controller_task_id;
END;

-- Agent 本地任务执行历史。
CREATE TABLE IF NOT EXISTS task_runs (
    run_id TEXT PRIMARY KEY,
    controller_task_id INTEGER NOT NULL,
    triggered_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    status TEXT NOT NULL,
    exit_code INTEGER,
    stdout TEXT,
    stderr TEXT,
    error_message TEXT,
    trigger_source TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_agent_task_runs_task_created
    ON task_runs(controller_task_id, created_at DESC);
