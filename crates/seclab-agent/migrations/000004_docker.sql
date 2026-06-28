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
