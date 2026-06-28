-- Docker 与系统指标缓存。
CREATE TABLE IF NOT EXISTS docker_metrics_summary (
    created_at INTEGER NOT NULL,
    cpu_percent REAL NOT NULL,
    memory_usage_bytes INTEGER NOT NULL,
    memory_limit_bytes INTEGER NOT NULL,
    memory_percent REAL NOT NULL,
    network_rx_bytes INTEGER NOT NULL,
    network_tx_bytes INTEGER NOT NULL,
    container_count INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_docker_metrics_summary_created_at
    ON docker_metrics_summary (created_at);

CREATE TABLE IF NOT EXISTS docker_metrics_container (
    container_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    cpu_percent REAL NOT NULL,
    memory_usage_bytes INTEGER NOT NULL,
    memory_limit_bytes INTEGER NOT NULL,
    memory_percent REAL NOT NULL,
    network_rx_bytes INTEGER NOT NULL,
    network_tx_bytes INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_docker_metrics_container_id_created_at
    ON docker_metrics_container (container_id, created_at);

CREATE TABLE IF NOT EXISTS system_metrics (
    created_at INTEGER PRIMARY KEY,
    load_avg_1 REAL NOT NULL,
    load_avg_5 REAL NOT NULL,
    load_avg_15 REAL NOT NULL,
    cpu_percent REAL NOT NULL,
    memory_used_bytes INTEGER NOT NULL,
    memory_total_bytes INTEGER NOT NULL,
    memory_percent REAL NOT NULL,
    disk_read_bytes INTEGER NOT NULL,
    disk_write_bytes INTEGER NOT NULL,
    network_rx_bytes INTEGER NOT NULL,
    network_tx_bytes INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_system_metrics_created_at
    ON system_metrics (created_at);
