-- Agent Docker 领域数据。

-- Docker Compose 项目注册。
CREATE TABLE IF NOT EXISTS docker_compose_projects (
    name TEXT PRIMARY KEY,
    display_name TEXT, -- 展示名称，允许和 Compose project name 分离。
    compose_dir TEXT NOT NULL,
    project_type TEXT NOT NULL DEFAULT 'docker',
    owner TEXT NOT NULL DEFAULT 'user', -- 项目归属，例如用户创建、套件创建或系统创建。
    labels_json TEXT NOT NULL DEFAULT '{}', -- Docker 资源来源和治理标签。
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TRIGGER IF NOT EXISTS set_docker_compose_projects_updated_at
AFTER UPDATE ON docker_compose_projects FOR EACH ROW
BEGIN
    UPDATE docker_compose_projects
       SET updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE name = OLD.name;
END;

-- Docker 宿主机资源汇总缓存。
CREATE TABLE IF NOT EXISTS docker_metrics_summary (
    created_at INTEGER PRIMARY KEY,
    cpu_core_percent REAL NOT NULL,
    cpu_host_percent REAL NOT NULL,
    memory_working_set_bytes INTEGER NOT NULL,
    memory_limit_bytes INTEGER NOT NULL,
    memory_percent REAL NOT NULL,
    running_container_count INTEGER NOT NULL,
    sampled_container_count INTEGER NOT NULL
);

-- Docker 容器资源时间序列；网络字段保存累计计数，由查询层计算速率。
CREATE TABLE IF NOT EXISTS docker_metrics_container (
    container_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    cpu_core_percent REAL NOT NULL,
    memory_working_set_bytes INTEGER NOT NULL,
    memory_limit_bytes INTEGER NOT NULL,
    memory_percent REAL NOT NULL,
    network_rx_bytes INTEGER NOT NULL,
    network_tx_bytes INTEGER NOT NULL,
    PRIMARY KEY (container_id, created_at)
);

CREATE INDEX IF NOT EXISTS idx_docker_metrics_container_created_at
    ON docker_metrics_container (created_at);
