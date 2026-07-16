-- 计划任务定义：保存 Master 管理的任务配置、资源归属和部署读模型。
CREATE TABLE scheduled_tasks (
    task_id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80),
    name_key TEXT NOT NULL,
    description TEXT CHECK(description IS NULL OR length(description) <= 500),
    node_id TEXT NOT NULL,
    command TEXT NOT NULL CHECK(length(command) BETWEEN 1 AND 65536),
    cron_expr TEXT NOT NULL,
    time_zone TEXT NOT NULL DEFAULT 'Asia/Shanghai',
    desired_state TEXT NOT NULL CHECK(desired_state IN ('enabled', 'disabled')),
    timeout_seconds INTEGER NOT NULL CHECK(timeout_seconds BETWEEN 1 AND 86400),
    prevent_overlap INTEGER NOT NULL CHECK(prevent_overlap IN (0, 1)),
    ownership_kind TEXT NOT NULL CHECK(ownership_kind IN ('custom', 'compose', 'suite', 'system')),
    owner_id TEXT,
    owner_name TEXT,
    manager_path TEXT,
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    deployment_status TEXT NOT NULL CHECK(deployment_status IN (
        'pending', 'applying', 'ready', 'waiting_for_node', 'failed', 'deleting', 'migrating'
    )),
    deployment_error_summary TEXT,
    last_synced_at TEXT,
    next_run_status TEXT NOT NULL CHECK(next_run_status IN (
        'scheduled', 'disabled', 'not_deployed', 'unavailable'
    )),
    next_run_at TEXT,
    last_run_id TEXT,
    deleted_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(node_id, name_key)
);

CREATE INDEX idx_scheduled_tasks_custom_list
    ON scheduled_tasks(ownership_kind, deleted_at, node_id, updated_at DESC, task_id);
CREATE INDEX idx_scheduled_tasks_deployment
    ON scheduled_tasks(deployment_status, updated_at, task_id);

-- 计划任务运行记录：保存每次触发的生命周期、可信操作人上下文和输出摘要。
CREATE TABLE scheduled_task_runs (
    run_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    trigger_source TEXT NOT NULL CHECK(trigger_source IN ('schedule', 'manual', 'batch')),
    status TEXT NOT NULL CHECK(status IN (
        'queued', 'starting', 'running', 'cancelling', 'succeeded', 'failed', 'timed_out', 'cancelled'
    )),
    phase TEXT,
    queued_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    exit_code INTEGER,
    error_code TEXT,
    error_summary TEXT,
    output_size_bytes INTEGER NOT NULL DEFAULT 0 CHECK(output_size_bytes >= 0),
    output_truncated INTEGER NOT NULL DEFAULT 0 CHECK(output_truncated IN (0, 1)),
    overlap_guard TEXT,
    actor_user_id INTEGER,
    actor_name TEXT,
    client_ip TEXT,
    trace_id TEXT,
    terminal_logged_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(task_id) REFERENCES scheduled_tasks(task_id) ON DELETE CASCADE
);

CREATE INDEX idx_scheduled_task_runs_task
    ON scheduled_task_runs(task_id, queued_at DESC, run_id DESC);
CREATE INDEX idx_scheduled_task_runs_active
    ON scheduled_task_runs(status, updated_at) WHERE status IN ('queued', 'starting', 'running', 'cancelling');
CREATE UNIQUE INDEX idx_scheduled_task_runs_overlap_guard
    ON scheduled_task_runs(overlap_guard)
    WHERE overlap_guard IS NOT NULL AND status IN ('queued', 'starting', 'running', 'cancelling');

-- 计划任务运行输出：保存 Master 可分页读取的有界命令输出。
CREATE TABLE scheduled_task_run_outputs (
    run_id TEXT PRIMARY KEY,
    content BLOB NOT NULL,
    size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0 AND size_bytes <= 262144),
    truncated INTEGER NOT NULL CHECK(truncated IN (0, 1)),
    updated_at TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES scheduled_task_runs(run_id) ON DELETE CASCADE
);

-- 脚本库：保存用户维护的可复用脚本内容和基础说明。
CREATE TABLE scripts (
    script_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TRIGGER set_scripts_updated_at
AFTER UPDATE ON scripts FOR EACH ROW
BEGIN
    UPDATE scripts
       SET updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE script_id = OLD.script_id;
END;
