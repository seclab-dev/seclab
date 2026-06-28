-- 协议仿真规则、实例、审计日志和规则包历史。
CREATE TABLE IF NOT EXISTS sim_rules (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    name_en TEXT NOT NULL,
    cve TEXT,
    category TEXT NOT NULL,
    description_zh TEXT NOT NULL,
    description_en TEXT NOT NULL,
    protocol TEXT NOT NULL,
    default_port INTEGER,
    config_yaml TEXT NOT NULL,
    source_type TEXT NOT NULL DEFAULT 'custom',
    source_package_id TEXT,
    rule_status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_sim_rules_status_type ON sim_rules (rule_status, source_type);

CREATE TRIGGER IF NOT EXISTS set_sim_rules_updated_at
AFTER UPDATE ON sim_rules FOR EACH ROW
BEGIN
    UPDATE sim_rules
       SET updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE id = OLD.id;
END;

CREATE TABLE IF NOT EXISTS sim_instances (
    instance_id TEXT PRIMARY KEY,
    node_id TEXT NOT NULL,
    rule_id INTEGER NOT NULL,
    listen_port INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'inactive', 'error')),
    error_message TEXT,
    pcap_status TEXT NOT NULL DEFAULT 'idle',
    pcap_start_time INTEGER,
    pcap_file_path TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (rule_id) REFERENCES sim_rules(id) ON DELETE CASCADE,
    UNIQUE(node_id, listen_port)
);

CREATE TRIGGER IF NOT EXISTS set_sim_instances_updated_at
AFTER UPDATE ON sim_instances FOR EACH ROW
BEGIN
    UPDATE sim_instances
       SET updated_at = (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE instance_id = OLD.instance_id;
END;

CREATE TABLE IF NOT EXISTS sim_logs (
    log_id INTEGER PRIMARY KEY AUTOINCREMENT,
    instance_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    client_ip TEXT NOT NULL,
    client_port INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    detail_summary TEXT NOT NULL,
    payload_hex TEXT,
    pcap_file_path TEXT,
    timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_sim_logs_node_id_timestamp ON sim_logs (node_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_sim_logs_instance_id ON sim_logs (instance_id);

CREATE TABLE IF NOT EXISTS sim_rule_packages (
    package_id TEXT NOT NULL,
    version TEXT NOT NULL,
    ruleset_format_version INTEGER NOT NULL,
    min_seclab_version TEXT NOT NULL,
    rule_count INTEGER NOT NULL,
    signature_hex TEXT NOT NULL,
    archive_sha256 TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'superseded')),
    imported_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (package_id, version)
);
