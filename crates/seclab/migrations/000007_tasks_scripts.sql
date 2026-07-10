-- 定时任务、任务运行记录与脚本库。
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT, -- 任务说明，用于展示用途和运行风险。
    agent_id TEXT NOT NULL,
    command TEXT NOT NULL,
    cron_expr TEXT NOT NULL,
    trigger_type TEXT NOT NULL DEFAULT 'cron', -- 触发类型，预留手动、一次性和事件触发任务。
    enabled INTEGER NOT NULL DEFAULT 1,
    timeout_secs INTEGER NOT NULL DEFAULT 60,
    no_overlap INTEGER NOT NULL DEFAULT 1,
    last_run_at INTEGER,
    next_run_at INTEGER,
    sync_status TEXT NOT NULL DEFAULT 'pending',
    sync_error TEXT,
    synced_at TEXT,
    revision INTEGER NOT NULL DEFAULT 1,
    metadata TEXT NOT NULL DEFAULT '{}', -- 任务调度、展示和扩展执行策略的结构化配置。
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_agent_id ON scheduled_tasks(agent_id);
CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_enabled_next_run ON scheduled_tasks(enabled, next_run_at);
CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_sync_status ON scheduled_tasks(sync_status);

CREATE TRIGGER IF NOT EXISTS set_scheduled_tasks_updated_at
AFTER UPDATE ON scheduled_tasks FOR EACH ROW
BEGIN
    UPDATE scheduled_tasks
       SET updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE id = OLD.id;
END;

CREATE TABLE IF NOT EXISTS task_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    agent_id TEXT NOT NULL,
    triggered_at INTEGER NOT NULL,
    started_at INTEGER,
    finished_at INTEGER,
    status TEXT NOT NULL,
    exit_code INTEGER,
    log_excerpt TEXT,
    error_message TEXT,
    run_id TEXT,
    trigger_source TEXT NOT NULL DEFAULT 'scheduler', -- 运行来源，例如调度器、手动触发或 Agent 回传。
    stdout TEXT, -- Agent 执行标准输出，长文本可按业务策略截断或归档。
    stderr TEXT, -- Agent 执行标准错误，长文本可按业务策略截断或归档。
    metadata TEXT NOT NULL DEFAULT '{}', -- 执行上下文、节点回传和展示扩展信息。
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (task_id) REFERENCES scheduled_tasks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_task_runs_task_id_created_at ON task_runs(task_id, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_task_runs_run_id ON task_runs(run_id);

CREATE TABLE IF NOT EXISTS scripts (
    script_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TRIGGER IF NOT EXISTS set_scripts_updated_at
AFTER UPDATE ON scripts FOR EACH ROW
BEGIN
    UPDATE scripts
       SET updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE script_id = OLD.script_id;
END;
