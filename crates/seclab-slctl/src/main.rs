//! `seclab-slctl` 命令行工具，用于管理和操作 SecLab 服务。
//! 支持服务的重启、状态查询、系统信息展示及服务的卸载等操作。

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const RESERVED_SAFE_ENTRY_PREFIXES: &[&str] = &[
    "api", "assets", "images", "favicon", "static", "public", "health", "metrics", "ws", "wss",
    "robots",
];

#[derive(Parser)]
#[command(name = "slctl")]
#[command(version, about = "SecLab control tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 重启指定的服务 (seclab 或 agent)
    Restart {
        /// 目标服务名称 (seclab 或 agent)
        target: String,
    },
    /// 显示服务运行状态
    Status,
    /// 显示节点和系统信息
    Info,
    /// 修改管理员密码
    Passwd {
        /// 同时修改用户名
        #[arg(long)]
        username: Option<String>,
        /// 新密码；不传则交互输入
        password: Option<String>,
    },
    /// 修改管理员用户名
    User {
        /// 新用户名
        #[arg(long)]
        username: String,
    },
    /// 管理安全入口
    Entry {
        /// 重新生成安全入口
        #[arg(long)]
        regenerate: bool,
        /// 设置自定义安全入口
        #[arg(long)]
        set: Option<String>,
        /// 关闭安全入口
        #[arg(long)]
        disable: bool,
    },
    /// 卸载服务，支持 --purge 清理数据
    Uninstall {
        /// 是否清理所有相关数据目录
        #[arg(long)]
        purge: bool,
    },
    /// 管理 slctl 自身
    #[command(name = "self")]
    SelfCmd {
        #[command(subcommand)]
        action: SelfAction,
    },
}

#[derive(Subcommand)]
enum SelfAction {
    /// 卸载 slctl 自身
    Uninstall,
}

/// 运行时上下文，缓存路径和特权状态。
struct Context {
    /// SecLab 主安装目录
    seclab_home: PathBuf,
    /// 配置目录
    config_dir: PathBuf,
    /// 数据库目录
    db_dir: PathBuf,
    /// 日志目录
    log_dir: PathBuf,
    /// 运行状态文件目录
    run_dir: PathBuf,
    /// Agent 的 Unix Socket 文件路径
    agent_socket: PathBuf,
    /// 当前是否需要提升权限运行特权指令。
    use_sudo: bool,
}

/// seclab 运行时监听配置。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeListenConfig {
    /// 实际监听地址。
    host: String,
    /// 实际监听端口。
    port: u16,
    /// 对外默认访问地址。
    public_host: Option<String>,
}

/// 检查当前运行的用户是否拥有 Root 权限。
fn check_privilege() -> Result<bool> {
    let uid = unsafe { libc::getuid() };
    if uid == 0 {
        return Ok(false);
    }
    eprintln!("slctl: must run as root");
    std::process::exit(1);
}

/// 动态检测当前可执行文件所在的路径，并推导出默认的 `SECLAB_HOME`。
fn get_detected_home() -> Result<PathBuf> {
    let current_exe = std::env::current_exe()?;
    let script_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Failed to get parent directory of executable"))?;

    let script_dir_str = script_dir.to_string_lossy();
    if script_dir_str == "/usr/local/bin"
        || script_dir_str == "/usr/bin"
        || script_dir.file_name().is_some_and(|n| n == "deploy")
    {
        Ok(PathBuf::from("/opt/seclab"))
    } else {
        Ok(script_dir.to_path_buf())
    }
}

/// 检测外部命令在当前系统环境变量 PATH 中是否存在。
fn command_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 构造服务实际在 Systemd 中注册的名称。
fn build_service_name(target: &str) -> String {
    if target == "seclab" {
        target.to_string()
    } else {
        format!("seclab-{}", target)
    }
}

/// 解析监听地址，返回 (IP, 端口) 的元组。
fn parse_listen_addr(addr: &str) -> (String, String) {
    if addr.is_empty() {
        return ("".to_string(), "".to_string());
    }
    // IPv6 格式像 [::1]:8080
    if addr.starts_with('[') && addr.rfind(']').is_some() {
        let r_bracket_idx = addr.rfind(']').unwrap();
        let host = &addr[0..=r_bracket_idx];
        let rest = &addr[r_bracket_idx + 1..];
        if let Some(port) = rest.strip_prefix(':') {
            return (host.to_string(), port.to_string());
        }
    }
    // 否则如果是带冒号的形式 host:port
    if let Some(colon_idx) = addr.rfind(':') {
        let host = &addr[0..colon_idx];
        let port = &addr[colon_idx + 1..];
        return (host.to_string(), port.to_string());
    }
    (addr.to_string(), "".to_string())
}

/// 将泛监听地址转换为可访问的本机地址。
fn normalize_listen_host(host: &str) -> String {
    match host.trim() {
        "" | "*" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
        value => value
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string(),
    }
}

/// 打印命令行工具用法。
fn print_usage() {
    eprintln!(
        "Usage:\n  slctl restart <seclab|agent>\n  slctl status\n  slctl info\n  slctl passwd [--username <username>] [password]\n  slctl user --username <username>\n  slctl entry [--regenerate|--set <entry>|--disable]\n  slctl uninstall [--purge]\n  slctl self uninstall"
    );
}

/// 读取一行交互输入。
fn prompt_line(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim_end_matches(['\r', '\n']).to_string())
}

/// 校验用户名。
fn validate_username_value(username: &str) -> Result<()> {
    let valid = !username.trim().is_empty()
        && username.len() <= 64
        && username
            .chars()
            .enumerate()
            .all(|(idx, ch)| ch.is_ascii_alphanumeric() || ch == '_' || (idx > 0 && ch == '-'));
    if !valid {
        return Err(anyhow::anyhow!(
            "Username must be 1-64 ASCII letters, digits, underscore, or hyphen"
        ));
    }
    Ok(())
}

/// 校验安全入口。
fn validate_safe_entry_value(value: &str) -> Result<()> {
    if !(8..=32).contains(&value.len()) || !value.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return Err(anyhow::anyhow!(
            "Safe entry must be 8-32 ASCII letters or digits"
        ));
    }
    let lower = value.to_ascii_lowercase();
    if RESERVED_SAFE_ENTRY_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return Err(anyhow::anyhow!(
            "Safe entry must not use a reserved path prefix"
        ));
    }
    Ok(())
}

/// 校验密码。
fn validate_password(password: &str, enforce_complexity: bool) -> Result<()> {
    if password.is_empty() {
        return Err(anyhow::anyhow!("Password must not be empty"));
    }
    if !enforce_complexity {
        if password.len() < 5 {
            return Err(anyhow::anyhow!(
                "Password length must be at least 5 characters"
            ));
        }
        return Ok(());
    }
    if !(8..=30).contains(&password.len()) {
        return Err(anyhow::anyhow!("Password length must be 8-30 characters"));
    }
    let has_letter = password.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_digit = password.chars().any(|ch| ch.is_ascii_digit());
    let has_special = password.chars().any(|ch| !ch.is_ascii_alphanumeric());
    let count = [has_letter, has_digit, has_special]
        .into_iter()
        .filter(|value| *value)
        .count();
    if count < 2 {
        return Err(anyhow::anyhow!(
            "Password must contain at least two character classes"
        ));
    }
    Ok(())
}

/// 生成随机安全入口。
fn generate_safe_entry() -> Result<String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?
        .read_exact(&mut bytes)
        .map_err(|err| anyhow::anyhow!("Failed to read random bytes: {}", err))?;
    let charset = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    Ok((0..16)
        .map(|idx| charset[bytes[idx] as usize % charset.len()] as char)
        .collect())
}

impl Context {
    /// 初始化运行时上下文，加载环境变量并推导目录路径。
    fn init() -> Result<Self> {
        let use_sudo = check_privilege()?;
        let detected_home = get_detected_home()?;

        let seclab_home = std::env::var("SECLAB_HOME")
            .map(PathBuf::from)
            .unwrap_or(detected_home);

        let config_dir = std::env::var("SECLAB_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| seclab_home.join("config"));

        let db_dir = std::env::var("SECLAB_DB_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| seclab_home.join("database"));

        let log_dir = std::env::var("SECLAB_LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| seclab_home.join("logs"));

        let run_dir = std::env::var("SECLAB_RUN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| seclab_home.join("run"));

        let agent_socket = std::env::var("SECLAB_AGENT_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|_| run_dir.join("seclab-agent.sock"));

        Ok(Self {
            seclab_home,
            config_dir,
            db_dir,
            log_dir,
            run_dir,
            agent_socket,
            use_sudo,
        })
    }

    /// 执行系统命令，如果 use_sudo 为真，则加上 sudo 前缀。
    fn run_command(&self, cmd: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
        let mut command = if self.use_sudo {
            let mut c = std::process::Command::new("sudo");
            c.arg(cmd);
            c
        } else {
            std::process::Command::new(cmd)
        };
        command.args(args);
        let status = command.status()?;
        Ok(status)
    }

    /// 静默执行系统命令，隐藏 stdout 与 stderr。如果 use_sudo 为真，则加上 sudo 前缀。
    fn run_command_silent(&self, cmd: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
        let mut command = if self.use_sudo {
            let mut c = std::process::Command::new("sudo");
            c.arg(cmd);
            c
        } else {
            std::process::Command::new(cmd)
        };
        command.args(args);
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        let status = command.status()?;
        Ok(status)
    }

    /// 执行系统命令并捕获标准输出。如果 use_sudo 为真，则加上 sudo 前缀。
    fn run_command_output(&self, cmd: &str, args: &[&str]) -> Result<String> {
        let mut command = if self.use_sudo {
            let mut c = std::process::Command::new("sudo");
            c.arg(cmd);
            c
        } else {
            std::process::Command::new(cmd)
        };
        command.args(args);
        let output = command.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        Ok(stdout)
    }

    /// 检查指定服务是否在 systemd 中已安装。
    fn service_installed(&self, target: &str) -> bool {
        let service = build_service_name(target);
        let status = self.run_command_silent(
            "systemctl",
            &["list-unit-files", &format!("{}.service", service)],
        );
        match status {
            Ok(s) => s.success(),
            Err(_) => false,
        }
    }

    /// 检测当前节点的角色 (seclab, agent, all, 或 unknown)。
    fn detect_node_role(&self) -> String {
        let role_file = self.config_dir.join("node.role");

        let role_content = if self.use_sudo {
            self.run_command_output("cat", &[&role_file.to_string_lossy()])
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        } else if role_file.exists() {
            std::fs::read_to_string(&role_file)
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        } else {
            "".to_string()
        };

        if role_content == "seclab" || role_content == "agent" || role_content == "all" {
            return role_content;
        }

        let has_seclab = self.service_installed("seclab");
        let has_agent = self.service_installed("agent");

        if has_seclab && has_agent {
            "all".to_string()
        } else if has_seclab {
            "seclab".to_string()
        } else if has_agent {
            "agent".to_string()
        } else {
            "unknown".to_string()
        }
    }

    /// 重启指定的 systemd 服务，并输出服务状态。
    fn restart_service(&self, target: &str) -> Result<()> {
        if target != "seclab" && target != "agent" {
            print_usage();
            std::process::exit(1);
        }

        if !command_exists("systemctl") {
            return Err(anyhow::anyhow!("systemctl not found"));
        }

        let service = build_service_name(target);
        self.run_command("systemctl", &["restart", &service])?;
        let status_output =
            self.run_command_output("systemctl", &["status", &service, "--no-pager"])?;
        print!("{}", status_output);
        Ok(())
    }

    /// 检查指定服务的运行状态并打印。
    fn status_service(&self, target: &str) -> Result<()> {
        if !command_exists("systemctl") {
            return Err(anyhow::anyhow!("systemctl not found"));
        }

        let display_name = if target == "seclab" {
            "seclab"
        } else {
            "seclab-agent"
        };

        if !self.service_installed(target) {
            println!(
                "  \x1b[90m○\x1b[0m  \x1b[1;37m{:<15}\x1b[0m : \x1b[90mNot Installed\x1b[0m",
                display_name
            );
            return Ok(());
        }

        let service = build_service_name(target);
        let status = self.run_command_silent("systemctl", &["is-active", "--quiet", &service])?;
        if status.success() {
            println!(
                "  \x1b[32m●\x1b[0m  \x1b[1;37m{:<15}\x1b[0m : \x1b[32mRunning\x1b[0m",
                display_name
            );
        } else {
            println!(
                "  \x1b[31m●\x1b[0m  \x1b[1;37m{:<15}\x1b[0m : \x1b[31mStopped\x1b[0m",
                display_name
            );
        }
        Ok(())
    }

    /// 展示当前节点和服务的详细信息。
    fn show_info(&self) -> Result<()> {
        let role = self.detect_node_role();
        println!("slctl: node role: {}", role);
        match role.as_str() {
            "agent" => {
                println!("slctl: seclab listen IP: N/A");
                println!("slctl: seclab listen port: N/A");
                println!("slctl: admin username: N/A");
            }
            "seclab" | "all" => {
                self.show_seclab_info()?;
            }
            _ => {
                println!("slctl: seclab listen IP: unknown");
                println!("slctl: seclab listen port: unknown");
                println!("slctl: admin username: unknown");
            }
        }
        Ok(())
    }

    /// 查询并显示 seclab 主服务的配置与运行详情（IP/Port/Admin）。
    fn show_seclab_info(&self) -> Result<()> {
        let service = build_service_name("seclab");
        let pid_str = self
            .run_command_output("systemctl", &["show", "-p", "MainPID", "--value", &service])
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let mut listen_ip = "unknown".to_string();
        let mut listen_port = "unknown".to_string();
        let runtime_config = self.load_runtime_listen_config();

        if !pid_str.is_empty() && pid_str != "0" && command_exists("ss") {
            let ss_output = self
                .run_command_output("ss", &["-ltnp"])
                .unwrap_or_default();
            let search_pattern = format!("pid={},", pid_str);
            for line in ss_output.lines() {
                if line.contains(&search_pattern) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let (ip, port) = parse_listen_addr(parts[3]);
                        if !ip.is_empty() {
                            listen_ip = ip;
                        }
                        if !port.is_empty() {
                            listen_port = port;
                        }
                    }
                    break;
                }
            }
        }

        if let Some(config) = runtime_config {
            listen_port = config.port.to_string();
            listen_ip = config
                .public_host
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| normalize_listen_host(&config.host));
        }

        let database_path = self.db_dir.join("seclab.db");
        let db_exists = if self.use_sudo {
            let status = self.run_command_silent("test", &["-f", &database_path.to_string_lossy()]);
            status.map(|s| s.success()).unwrap_or(false)
        } else {
            database_path.is_file()
        };

        let admin_username = if !db_exists {
            "unknown (Database not found)".to_string()
        } else {
            match self.get_admin_username_from_db() {
                Ok(name) => name,
                Err(e) => {
                    let err_msg = e.to_string();
                    if err_msg.contains("Permission denied") {
                        "unknown (Permission denied, run as root)".to_string()
                    } else {
                        "unknown (Access failed)".to_string()
                    }
                }
            }
        };
        let safe_entry = self.get_safe_entry_from_db().unwrap_or_default();

        println!("slctl: seclab listen IP: {}", listen_ip);
        println!("slctl: seclab listen port: {}", listen_port);
        println!("slctl: admin username: {}", admin_username);
        if safe_entry.trim().is_empty() {
            println!("slctl: safe entry: disabled");
        } else {
            println!("slctl: safe entry: {}", safe_entry.trim());
            println!(
                "slctl: panel login URL: https://{}:{}/{}",
                listen_ip,
                listen_port,
                safe_entry.trim()
            );
        }
        Ok(())
    }

    /// 读取运行时监听配置。
    fn load_runtime_listen_config(&self) -> Option<RuntimeListenConfig> {
        let path = self.config_dir.join("runtime-listen.json");
        let raw = if self.use_sudo {
            self.run_command_output("cat", &[&path.to_string_lossy()])
                .ok()?
        } else {
            std::fs::read_to_string(path).ok()?
        };
        serde_json::from_str(&raw).ok()
    }

    /// 安全删除文件或目录（如果存在）。
    fn remove_file_if_exists(&self, path: &str) -> Result<()> {
        let exists = if self.use_sudo {
            let status_e = self.run_command_silent("test", &["-e", path]);
            let status_l = self.run_command_silent("test", &["-L", path]);
            status_e.map(|s| s.success()).unwrap_or(false)
                || status_l.map(|s| s.success()).unwrap_or(false)
        } else {
            let p = Path::new(path);
            p.exists() || p.is_symlink()
        };

        if exists {
            self.run_command_silent("rm", &["-rf", path])?;
        }
        Ok(())
    }

    /// 彻底清理数据目录与缓存。
    fn purge_common_dirs(&self) -> Result<()> {
        self.remove_file_if_exists(&self.db_dir.to_string_lossy())?;
        self.remove_file_if_exists(&self.log_dir.to_string_lossy())?;
        self.remove_file_if_exists(&self.agent_socket.to_string_lossy())?;
        self.remove_file_if_exists(&self.run_dir.to_string_lossy())?;
        self.remove_file_if_exists(&self.seclab_home.to_string_lossy())?;
        self.remove_file_if_exists("/etc/seclab")?;
        self.remove_file_if_exists("/var/lib/seclab")?;
        self.remove_file_if_exists("/var/log/seclab")?;
        self.remove_file_if_exists("/run/seclab")?;
        Ok(())
    }

    /// 卸载指定的服务。
    fn uninstall_service(&self, target: &str, purge: bool) -> Result<()> {
        if !command_exists("systemctl") {
            return Err(anyhow::anyhow!("systemctl not found"));
        }

        let service = build_service_name(target);
        println!("slctl: start uninstall {}", target);

        let _ = self.run_command_silent("systemctl", &["stop", &service]);
        let _ = self.run_command_silent("systemctl", &["disable", &service]);

        self.remove_file_if_exists(&format!("/etc/systemd/system/{}.service", service))?;
        self.remove_file_if_exists(&format!("/usr/lib/systemd/system/{}.service", service))?;
        self.remove_file_if_exists(&format!("/lib/systemd/system/{}.service", service))?;
        self.remove_file_if_exists(&format!(
            "/etc/systemd/system/multi-user.target.wants/{}.service",
            service
        ))?;

        let _ = self.run_command_silent("systemctl", &["daemon-reload"]);
        let _ = self.run_command_silent("systemctl", &["reset-failed", &service]);

        self.remove_file_if_exists(&format!("/usr/local/bin/{}", service))?;
        self.remove_file_if_exists(&format!("/usr/bin/{}", service))?;

        if target == "agent" {
            self.remove_file_if_exists(&self.config_dir.join("agent.toml").to_string_lossy())?;
            self.remove_file_if_exists(
                &self.config_dir.join("agent.install_dir").to_string_lossy(),
            )?;
            self.remove_file_if_exists(&self.config_dir.join("node.role").to_string_lossy())?;
            self.remove_file_if_exists(&self.seclab_home.join("agent").to_string_lossy())?;

            self.remove_file_if_exists(&self.db_dir.join("agent.db").to_string_lossy())?;
            self.remove_file_if_exists(&self.db_dir.join("agent.db-shm").to_string_lossy())?;
            self.remove_file_if_exists(&self.db_dir.join("agent.db-wal").to_string_lossy())?;

            if purge {
                self.purge_common_dirs()?;
            }
        } else {
            self.remove_file_if_exists(&self.config_dir.join("seclab.toml").to_string_lossy())?;

            if purge {
                self.purge_common_dirs()?;
            }
        }

        println!("slctl: {} uninstall completed", target);
        Ok(())
    }

    /// 卸载 `slctl` 工具自身。
    fn uninstall_self(&self) -> Result<()> {
        self.remove_file_if_exists("/usr/local/bin/slctl")?;
        self.remove_file_if_exists("/usr/bin/slctl")?;
        println!("slctl: slctl uninstall completed");
        Ok(())
    }

    /// 打开主控数据库。
    fn open_database(&self) -> Result<rusqlite::Connection> {
        let database_path = self.db_dir.join("seclab.db");
        rusqlite::Connection::open(&database_path).map_err(|e| {
            anyhow::anyhow!("Cannot access database {}: {}", database_path.display(), e)
        })
    }

    /// 从数据库中获取管理员用户名。
    fn get_admin_username_from_db(&self) -> Result<String> {
        let conn = self.open_database()?;
        let mut stmt = conn.prepare("SELECT username FROM users ORDER BY id LIMIT 1;")?;
        let username: String = stmt.query_row([], |row| row.get(0))?;
        Ok(username)
    }

    /// 确保系统配置单行存在。
    fn ensure_system_config(&self, conn: &rusqlite::Connection) -> Result<()> {
        conn.execute("INSERT OR IGNORE INTO system_config (id) VALUES (1);", [])?;
        Ok(())
    }

    /// 从数据库中读取安全入口。
    fn get_safe_entry_from_db(&self) -> Result<String> {
        let conn = self.open_database()?;
        self.ensure_system_config(&conn)?;
        let value = conn.query_row(
            "SELECT safe_entry FROM system_config WHERE id = 1;",
            [],
            |row| row.get::<_, String>(0),
        )?;
        Ok(value)
    }

    /// 写入安全入口。
    fn set_safe_entry_in_db(&self, value: &str) -> Result<()> {
        let conn = self.open_database()?;
        self.ensure_system_config(&conn)?;
        conn.execute(
            "UPDATE system_config SET safe_entry = ?1 WHERE id = 1;",
            rusqlite::params![value],
        )?;
        Ok(())
    }

    /// 修改管理员密码，可同时修改用户名。
    fn passwd(&self, username: Option<&str>, new_password: &str) -> Result<()> {
        validate_password(new_password, self.password_complexity_enabled()?)?;
        let password_hash = bcrypt::hash(new_password, bcrypt::DEFAULT_COST)
            .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?;
        let conn = self.open_database()?;
        let affected = if let Some(username) = username {
            validate_username_value(username)?;
            conn.execute(
                "UPDATE users SET username = ?1, password_hash = ?2 WHERE id = (SELECT id FROM users ORDER BY id LIMIT 1);",
                rusqlite::params![username, password_hash],
            )?
        } else {
            conn.execute(
                "UPDATE users SET password_hash = ?1 WHERE id = (SELECT id FROM users ORDER BY id LIMIT 1);",
                rusqlite::params![password_hash],
            )?
        };
        if affected == 0 {
            return Err(anyhow::anyhow!("No admin user found in database."));
        }
        println!("slctl: password updated");
        Ok(())
    }

    /// 修改管理员用户名。
    fn update_username(&self, username: &str) -> Result<()> {
        validate_username_value(username)?;
        let conn = self.open_database()?;
        let affected = conn.execute(
            "UPDATE users SET username = ?1 WHERE id = (SELECT id FROM users ORDER BY id LIMIT 1);",
            rusqlite::params![username],
        )?;
        if affected == 0 {
            return Err(anyhow::anyhow!("No admin user found in database."));
        }
        println!("slctl: username updated to '{}'", username);
        Ok(())
    }

    /// 当前密码复杂度开关。
    fn password_complexity_enabled(&self) -> Result<bool> {
        let conn = self.open_database()?;
        self.ensure_system_config(&conn)?;
        let value: i64 = conn.query_row(
            "SELECT password_complexity FROM system_config WHERE id = 1;",
            [],
            |row| row.get(0),
        )?;
        Ok(value != 0)
    }

    /// 查看或修改安全入口。
    fn entry(&self, regenerate: bool, set: Option<String>, disable: bool) -> Result<()> {
        let action_count = [regenerate, set.is_some(), disable]
            .into_iter()
            .filter(|value| *value)
            .count();
        if action_count > 1 {
            return Err(anyhow::anyhow!(
                "Only one of --regenerate, --set, or --disable can be used"
            ));
        }

        if regenerate {
            let value = generate_safe_entry()?;
            self.set_safe_entry_in_db(&value)?;
            println!("slctl: safe entry updated: {}", value);
            return Ok(());
        }
        if let Some(value) = set {
            validate_safe_entry_value(&value)?;
            self.set_safe_entry_in_db(&value)?;
            println!("slctl: safe entry updated: {}", value);
            return Ok(());
        }
        if disable {
            self.set_safe_entry_in_db("")?;
            println!("slctl: safe entry disabled");
            return Ok(());
        }

        let entry = self.get_safe_entry_from_db()?;
        if entry.trim().is_empty() {
            println!("slctl: safe entry: disabled");
        } else {
            println!("slctl: safe entry: {}", entry.trim());
        }
        Ok(())
    }
}

/// 分发并执行命令行指令。
fn run_app(cli: Cli, context: Context) -> Result<()> {
    match cli.command {
        Commands::Restart { target } => {
            context.restart_service(&target)?;
        }
        Commands::Status => {
            println!("\x1b[1;36mSecLab Service Status\x1b[0m");
            println!("\x1b[90m----------------------------------------\x1b[0m");
            let role = context.detect_node_role();
            match role.as_str() {
                "agent" => {
                    context.status_service("agent")?;
                }
                "seclab" => {
                    context.status_service("seclab")?;
                }
                _ => {
                    context.status_service("seclab")?;
                    context.status_service("agent")?;
                }
            }
            println!("\x1b[90m----------------------------------------\x1b[0m");
        }
        Commands::Info => {
            context.show_info()?;
        }
        Commands::Passwd { username, password } => {
            let password = match password {
                Some(value) => value,
                None => {
                    let first = prompt_line("New password: ")?;
                    let second = prompt_line("Confirm password: ")?;
                    if first != second {
                        return Err(anyhow::anyhow!("Password confirmation does not match"));
                    }
                    first
                }
            };
            context.passwd(username.as_deref(), &password)?;
        }
        Commands::User { username } => {
            context.update_username(&username)?;
        }
        Commands::Entry {
            regenerate,
            set,
            disable,
        } => {
            context.entry(regenerate, set, disable)?;
        }
        Commands::Uninstall { purge } => {
            context.uninstall_service("agent", purge)?;
            context.uninstall_service("seclab", purge)?;
        }
        Commands::SelfCmd { action } => match action {
            SelfAction::Uninstall => {
                context.uninstall_self()?;
            }
        },
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => match e.kind() {
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                e.exit();
            }
            _ => {
                print_usage();
                std::process::exit(1);
            }
        },
    };

    let context = match Context::init() {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("slctl error: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = run_app(cli, context) {
        eprintln!("slctl: {}", e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_password, validate_safe_entry_value};

    #[test]
    fn validates_safe_entry_format_and_reserved_prefixes() {
        assert!(validate_safe_entry_value("Xm9Kp2Qs").is_ok());
        assert!(validate_safe_entry_value("abc1234").is_err());
        assert!(validate_safe_entry_value("abc1234!").is_err());
        assert!(validate_safe_entry_value("api123456").is_err());
        assert!(validate_safe_entry_value("AssetsLogin").is_err());
    }

    #[test]
    fn validates_password_minimum_length_without_complexity() {
        assert!(validate_password("", false).is_err());
        assert!(validate_password("1234", false).is_err());
        assert!(validate_password("12345", false).is_ok());
    }

    #[test]
    fn validates_password_complexity_when_enabled() {
        assert!(validate_password("abcdefg1", true).is_ok());
        assert!(validate_password("abcdefgh", true).is_err());
        assert!(validate_password("abc1", true).is_err());
    }
}
