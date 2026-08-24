#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -t 1 ]]; then
  COLOR_RESET="\033[0m"
  COLOR_DIM="\033[2m"
  COLOR_GREEN="\033[32m"
  COLOR_YELLOW="\033[33m"
  COLOR_RED="\033[31m"
  COLOR_CYAN="\033[36m"
  COLOR_BOLD="\033[1m"
else
  COLOR_RESET=""
  COLOR_DIM=""
  COLOR_GREEN=""
  COLOR_YELLOW=""
  COLOR_RED=""
  COLOR_CYAN=""
  COLOR_BOLD=""
fi

log() {
  echo -e "${COLOR_DIM}[$(date '+%Y-%m-%d %H:%M:%S')]${COLOR_RESET} ${COLOR_GREEN}$*${COLOR_RESET}"
}

log_section() {
  echo -e "${COLOR_DIM}[$(date '+%Y-%m-%d %H:%M:%S')]${COLOR_RESET} ${COLOR_BOLD}${COLOR_CYAN}$*${COLOR_RESET}"
}

warn() {
  echo -e "${COLOR_DIM}[$(date '+%Y-%m-%d %H:%M:%S')]${COLOR_RESET} ${COLOR_YELLOW}$*${COLOR_RESET}"
}

fail() {
  echo -e "${COLOR_DIM}[$(date '+%Y-%m-%d %H:%M:%S')]${COLOR_RESET} ${COLOR_BOLD}${COLOR_RED}Install failed: $*${COLOR_RESET}" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "command not found: $1"
}

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

capture_running_suite_socket_containers() {
  local socket_path="$1"
  RUNNING_SUITE_SOCKET_CONTAINERS=()
  command_exists docker || return 0

  local container_id
  while IFS= read -r container_id; do
    [[ -n "$container_id" ]] || continue
    local mount_sources
    mount_sources="$($PREFIX docker inspect --format '{{range .Mounts}}{{println .Source}}{{end}}' "$container_id" 2>/dev/null || true)"
    if grep -Fxq "$socket_path" <<<"$mount_sources"; then
      RUNNING_SUITE_SOCKET_CONTAINERS+=("$container_id")
    fi
  done < <($PREFIX docker ps --quiet --filter 'label=seclab.owner=suite')
}

stop_running_suite_socket_containers() {
  if (( ${#RUNNING_SUITE_SOCKET_CONTAINERS[@]} == 0 )); then
    return 0
  fi
  log "stop suite containers bound to the current Agent socket: ${#RUNNING_SUITE_SOCKET_CONTAINERS[@]}"
  $PREFIX docker stop "${RUNNING_SUITE_SOCKET_CONTAINERS[@]}" >/dev/null
}

wait_for_agent_socket() {
  local socket_path="$1"
  for _ in {1..30}; do
    if $PREFIX systemctl is-active --quiet seclab-agent && $PREFIX test -S "$socket_path"; then
      return 0
    fi
    sleep 1
  done
  fail "seclab-agent did not create a ready Unix socket within 30 seconds: $socket_path"
}

wait_for_seclab_listener() {
  local port="$1"
  for _ in {1..30}; do
    if $PREFIX systemctl is-active --quiet seclab \
      && (exec 3<>"/dev/tcp/127.0.0.1/${port}") 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  fail "seclab did not create a ready HTTPS listener within 30 seconds: 127.0.0.1:${port}"
}

restore_running_suite_socket_containers() {
  if (( ${#RUNNING_SUITE_SOCKET_CONTAINERS[@]} == 0 )); then
    return 0
  fi
  log "restore suite containers with the new Agent socket: ${#RUNNING_SUITE_SOCKET_CONTAINERS[@]}"
  $PREFIX docker start "${RUNNING_SUITE_SOCKET_CONTAINERS[@]}" >/dev/null
}

random_chars() {
  local length="$1"
  local charset="$2"
  local output=""
  local chunk=""
  while (( ${#output} < length )); do
    if command -v openssl >/dev/null 2>&1; then
      chunk="$(openssl rand -base64 96)"
    else
      chunk="$(dd if=/dev/urandom bs=96 count=1 2>/dev/null | base64)"
    fi
    output="${output}$(printf '%s' "$chunk" | LC_ALL=C tr -dc "$charset")"
  done
  printf '%s' "${output:0:length}"
}

validate_safe_entry() {
  local value="$1"
  [[ "$value" =~ ^[A-Za-z0-9]{8,32}$ ]] || return 1
  local lower
  lower="$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')"
  local prefix
  for prefix in api assets images favicon static public health metrics ws wss robots; do
    [[ "$lower" != "$prefix"* ]] || return 1
  done
  return 0
}

validate_username() {
  local value="$1"
  [[ "$value" =~ ^[A-Za-z0-9_][A-Za-z0-9_-]{0,63}$ ]]
}

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf '%s' "$value"
}

prompt_yes_no() {
  local prompt="$1"
  local reply=""
  local formatted_prompt
  formatted_prompt="${COLOR_DIM}[$(date '+%Y-%m-%d %H:%M:%S')]${COLOR_RESET} ${COLOR_CYAN}${prompt}${COLOR_RESET}"
  if [[ -t 0 ]]; then
    echo -ne "${formatted_prompt}" >&2
    read -r reply
  elif [[ -r /dev/tty ]]; then
    echo -ne "${formatted_prompt}" >&2
    read -r reply </dev/tty
  else
    return 1
  fi
  case "$reply" in
    y|Y|yes|YES)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

prompt_yes_no_default_yes() {
  local prompt="$1"
  local reply=""
  local formatted_prompt
  formatted_prompt="${COLOR_DIM}[$(date '+%Y-%m-%d %H:%M:%S')]${COLOR_RESET} ${COLOR_CYAN}${prompt}${COLOR_RESET}"
  if [[ -t 0 ]]; then
    echo -ne "${formatted_prompt}" >&2
    read -r reply
  elif [[ -r /dev/tty ]]; then
    echo -ne "${formatted_prompt}" >&2
    read -r reply </dev/tty
  else
    return 1
  fi
  case "$reply" in
    ""|y|Y|yes|YES)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

read_tty_input() {
  local prompt="$1"
  local reply=""
  local formatted_prompt
  formatted_prompt="${COLOR_DIM}[$(date '+%Y-%m-%d %H:%M:%S')]${COLOR_RESET} ${COLOR_CYAN}${prompt}${COLOR_RESET}"
  if [[ -t 0 ]]; then
    echo -ne "${formatted_prompt}" >&2
    read -r reply
  elif [[ -r /dev/tty ]]; then
    echo -ne "${formatted_prompt}" >&2
    read -r reply </dev/tty
  else
    return 1
  fi
  echo "$reply"
}

read_optional_path() {
  local prompt="$1"
  local default_value="$2"
  local reply
  reply="$(read_tty_input "${prompt}(${default_value}): ")" || echo "${default_value}"
  if [[ -z "$reply" ]]; then
    echo "${default_value}"
    return
  fi
  echo "$reply"
}

detect_lan_ipv4() {
  if command_exists ip; then
    local route_source
    route_source="$(ip route get 223.5.5.5 2>/dev/null | sed -n 's/.* src \([0-9.]*\).*/\1/p' | head -n 1)"
    if [[ -n "$route_source" && "$route_source" != "127."* ]]; then
      echo "$route_source"
      return 0
    fi
    route_source="$(ip route get 8.8.8.8 2>/dev/null | sed -n 's/.* src \([0-9.]*\).*/\1/p' | head -n 1)"
    if [[ -n "$route_source" && "$route_source" != "127."* ]]; then
      echo "$route_source"
      return 0
    fi
    route_source="$(ip -4 -o addr show scope global 2>/dev/null | awk '{print $4}' | cut -d/ -f1 | head -n 1)"
    if [[ -n "$route_source" && "$route_source" != "127."* ]]; then
      echo "$route_source"
      return 0
    fi
  fi
  if command_exists hostname; then
    local host_ip
    host_ip="$(hostname -I 2>/dev/null | tr ' ' '\n' | grep -E '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$' | grep -v '^127\.' | head -n 1)"
    if [[ -n "$host_ip" ]]; then
      echo "$host_ip"
      return 0
    fi
  fi
  echo "127.0.0.1"
}

read_default_callback_host() {
  local default_host="$1"
  local port="$2"
  local reply
  local prompt
  echo -e "${COLOR_DIM}[$(date '+%Y-%m-%d %H:%M:%S')] default callback URL preview: https://${default_host}:${port}${COLOR_RESET}" >&2
  prompt="Default controller callback host (${default_host}): "
  reply="$(read_tty_input "$prompt")" || echo "$default_host"
  if [[ -z "$reply" ]]; then
    echo "$default_host"
    return
  fi
  echo "$reply"
}

validate_callback_host() {
  local host="$1"
  [[ -n "$host" ]] || fail "default callback host cannot be empty"
  [[ "$host" != *"://"* ]] || fail "default callback host must be an IP or domain, without scheme"
  [[ "$host" != *"/"* ]] || fail "default callback host must not include path"
  [[ "$host" != *":"* ]] || fail "default callback host must not include port"
}

normalize_abs_path() {
  local path="$1"
  [[ -n "$path" ]] || fail "path cannot be empty"
  [[ "$path" == /* ]] || fail "path must be absolute: $path"
  while [[ "$path" != "/" && "$path" == */ ]]; do
    path="${path%/}"
  done
  echo "$path"
}

is_firewalld_active() {
  if ! command_exists firewall-cmd; then
    return 1
  fi
  if systemctl is-active --quiet firewalld 2>/dev/null; then
    return 0
  fi
  return 1
}

is_ufw_active() {
  if ! command_exists ufw; then
    return 1
  fi
  ufw status 2>/dev/null | grep -q "^Status: active"
}

open_port_firewalld() {
  local prefix="$1"
  local port="$2"
  ${prefix} firewall-cmd --permanent --add-port="${port}/tcp" >/dev/null
  ${prefix} firewall-cmd --reload >/dev/null
}

open_port_ufw() {
  local prefix="$1"
  local port="$2"
  ${prefix} ufw allow "${port}/tcp" >/dev/null
}

maybe_open_firewall_port() {
  local prefix="$1"
  local port="$2"
  local handled="false"

  if is_firewalld_active; then
    handled="true"
    warn "firewalld is active. SecLab port ${port}/tcp may be blocked."
    if prompt_yes_no "Open ${port}/tcp in firewalld now? [y/N] "; then
      if open_port_firewalld "$prefix" "$port"; then
        log "firewalld rule added: ${port}/tcp"
      else
        warn "failed to add firewalld rule for ${port}/tcp"
      fi
    else
      warn "skip firewall change for firewalld"
    fi
  fi

  if is_ufw_active; then
    handled="true"
    warn "ufw is active. SecLab port ${port}/tcp may be blocked."
    if prompt_yes_no "Open ${port}/tcp in ufw now? [y/N] "; then
      if open_port_ufw "$prefix" "$port"; then
        log "ufw rule added: ${port}/tcp"
      else
        warn "failed to add ufw rule for ${port}/tcp"
      fi
    else
      warn "skip firewall change for ufw"
    fi
  fi

  if [[ "$handled" != "true" ]]; then
    log "no active firewalld/ufw detected"
  fi
}

port_in_use() {
  local port="$1"
  if command_exists ss; then
    ss -ltn 2>/dev/null | awk '{print $4}' | grep -E -q "(^|[:.])${port}$"
    return $?
  fi
  if command_exists netstat; then
    netstat -ltn 2>/dev/null | awk '{print $4}' | grep -E -q "(^|[:.])${port}$"
    return $?
  fi
  if command_exists lsof; then
    lsof -iTCP -sTCP:LISTEN -P -n 2>/dev/null | grep -E -q "[:.]${port}[[:space:]]"
    return $?
  fi
  warn "ss/netstat/lsof not found, skip port conflict check"
  return 1
}

sudo_prefix() {
  if [[ "$(id -u)" -eq 0 ]]; then
    echo ""
    return
  fi
  if command -v sudo >/dev/null 2>&1; then
    echo "sudo"
    return
  fi
  fail "root privileges or sudo are required"
}

SECLAB_HOST="::"
SECLAB_PORT="7310"
SECLAB_PORT_FROM_ARG="false"
SECLAB_PUBLIC_HOST=""
SECLAB_PUBLIC_HOST_FROM_ARG="false"
SECLAB_HOME="/opt/seclab"
PREFIX="$(sudo_prefix)"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --seclab-host)
      [[ $# -ge 2 ]] || fail "missing value for --seclab-host"
      SECLAB_HOST="$2"
      shift 2
      ;;
    --seclab-port)
      [[ $# -ge 2 ]] || fail "missing value for --seclab-port"
      SECLAB_PORT="$2"
      SECLAB_PORT_FROM_ARG="true"
      shift 2
      ;;
    --seclab-public-host)
      [[ $# -ge 2 ]] || fail "missing value for --seclab-public-host"
      SECLAB_PUBLIC_HOST="$2"
      SECLAB_PUBLIC_HOST_FROM_ARG="true"
      shift 2
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

# Check if already installed before any user prompts
installed="false"
if [[ -f "/etc/systemd/system/seclab.service" || -f "/etc/systemd/system/seclab-agent.service" ]]; then
  installed="true"
elif [[ -f "/opt/seclab/config/node.role" ]]; then
  installed="true"
fi

if [[ "$installed" == "true" ]]; then
  if ! prompt_yes_no "SecLab installation or service was detected. Overwrite existing installation? (y/N): "; then
    log "Installation cancelled."
    exit 0
  fi
fi

if [[ "$SECLAB_PORT_FROM_ARG" != "true" ]]; then
  while true; do
    INPUT_SECLAB_PORT="$(read_tty_input "Use default SecLab port (${SECLAB_PORT})? : ")" || fail "failed to read port input"
    if [[ -z "$INPUT_SECLAB_PORT" ]]; then
      break
    fi
    if [[ "$INPUT_SECLAB_PORT" =~ ^[0-9]+$ ]] && (( INPUT_SECLAB_PORT >= 1 && INPUT_SECLAB_PORT <= 65535 )); then
      SECLAB_PORT="$INPUT_SECLAB_PORT"
      break
    fi
    warn "invalid port: $INPUT_SECLAB_PORT, expected 1-65535"
  done
fi

if ! [[ "$SECLAB_PORT" =~ ^[0-9]+$ ]]; then
  fail "SecLab port must be numeric: $SECLAB_PORT"
fi
if (( SECLAB_PORT < 1 || SECLAB_PORT > 65535 )); then
  fail "SecLab port must be in range 1-65535: $SECLAB_PORT"
fi

if [[ "$SECLAB_PUBLIC_HOST_FROM_ARG" != "true" ]]; then
  DETECTED_SECLAB_PUBLIC_HOST="$(detect_lan_ipv4)"
  SECLAB_PUBLIC_HOST="$(read_default_callback_host "$DETECTED_SECLAB_PUBLIC_HOST" "$SECLAB_PORT")"
fi
validate_callback_host "$SECLAB_PUBLIC_HOST"

SECLAB_HOME="$(normalize_abs_path "$(read_optional_path "Installation directory" "$SECLAB_HOME")")"
SECLAB_CONFIG_DIR="${SECLAB_HOME}/config"
SECLAB_DB_DIR="${SECLAB_HOME}/database"
SECLAB_LOG_DIR="${SECLAB_HOME}/logs"
SECLAB_RUN_DIR="${SECLAB_HOME}/run"
SECLAB_AGENT_SOCKET="${SECLAB_RUN_DIR}/seclab-agent.sock"
RUNNING_SUITE_SOCKET_CONTAINERS=()

DEFAULT_ADMIN_USERNAME="seclab"
DEFAULT_ADMIN_PASSWORD="$(random_chars 16 'A-Za-z0-9!@#$%^&*')"
DEFAULT_SAFE_ENTRY="$(random_chars 16 'A-Za-z0-9')"

ADMIN_USERNAME="$(read_tty_input "Admin username [${DEFAULT_ADMIN_USERNAME}]: ")" || ADMIN_USERNAME=""
if [[ -z "$ADMIN_USERNAME" ]]; then
  ADMIN_USERNAME="$DEFAULT_ADMIN_USERNAME"
fi
validate_username "$ADMIN_USERNAME" || fail "Admin username must be 1-64 characters and contain only letters, digits, underscore, or hyphen"

ADMIN_PASSWORD="$(read_tty_input "Admin password [generated]: ")" || ADMIN_PASSWORD=""
if [[ -z "$ADMIN_PASSWORD" ]]; then
  ADMIN_PASSWORD="$DEFAULT_ADMIN_PASSWORD"
fi
[[ -n "$ADMIN_PASSWORD" ]] || fail "Admin password must not be empty"
((${#ADMIN_PASSWORD} >= 5)) || fail "Admin password must be at least 5 characters"

SAFE_ENTRY="$(read_tty_input "Safe login entry [${DEFAULT_SAFE_ENTRY}]: ")" || SAFE_ENTRY=""
if [[ -z "$SAFE_ENTRY" ]]; then
  SAFE_ENTRY="$DEFAULT_SAFE_ENTRY"
fi
validate_safe_entry "$SAFE_ENTRY" || fail "Safe entry must be 8-32 ASCII letters or digits and must not use a reserved path prefix"

if [[ "$installed" == "true" ]]; then
  capture_running_suite_socket_containers "$SECLAB_AGENT_SOCKET"
  stop_running_suite_socket_containers
  log "Stopping existing SecLab services for overwrite..."
  $PREFIX systemctl stop seclab-agent >/dev/null 2>&1 || true
  $PREFIX systemctl stop seclab >/dev/null 2>&1 || true
fi

if port_in_use "$SECLAB_PORT"; then
  fail "SecLab port is already in use: $SECLAB_PORT"
fi

log_section "== SecLab install started"
log "system: $(uname -s) $(uname -m)"
if [[ -f /etc/os-release ]]; then
  OS_NAME="$(. /etc/os-release && echo "${PRETTY_NAME:-}")"
  if [[ -n "$OS_NAME" ]]; then
    log "distro: $OS_NAME"
  fi
fi

if ! command -v docker >/dev/null 2>&1; then
  warn "Docker not detected"
  if prompt_yes_no "Try to install Docker? [y/N] "; then
    warn "Docker auto-install is not implemented yet"
    fail "please install Docker manually and retry"
  else
    warn "continue without Docker"
  fi
else
  DOCKER_VERSION="$(docker --version 2>/dev/null || true)"
  if [[ -n "$DOCKER_VERSION" ]]; then
    log "Docker installed: $DOCKER_VERSION"
  else
    log "Docker installed"
  fi
fi

log "checking systemd..."
require_cmd systemctl
log "checking tar..."
require_cmd tar
# PREFIX was initialized above
log "checking firewall status..."
maybe_open_firewall_port "$PREFIX" "$SECLAB_PORT"

install_from_tarball() {
  local component="$1"  # "controller" or "agent"
  local name="$2"       # "seclab" or "seclab-agent"
  local tarball
  if [[ "$component" == "agent" ]]; then
    tarball="$(find "$SCRIPT_DIR" -maxdepth 1 -type f -name "seclab-agent-*.tar.gz" | sort | head -n 1)"
  else
    tarball="$(
      find "$SCRIPT_DIR" -maxdepth 1 -type f \
        -name "seclab-*.tar.gz" \
        ! -name "seclab-agent-*.tar.gz" \
        ! -name "seclab-[0-9]*.tar.gz" \
        | sort \
        | head -n 1
    )"
  fi
  [[ -n "$tarball" && -f "$tarball" ]] || fail "missing component package for $component"

  local temp_bin_dir
  temp_bin_dir="$(mktemp -d)"
  tar -xzf "$tarball" -C "$temp_bin_dir"

  local source="$temp_bin_dir/$name"
  local target="/usr/local/bin/$name"
  local link="/usr/bin/$name"
  [[ -x "$source" ]] || { rm -rf "$temp_bin_dir"; fail "missing binary in package: $name"; }
  $PREFIX install -m 0755 "$source" "$target"
  $PREFIX ln -sf "$target" "$link"
  rm -rf "$temp_bin_dir"
}

write_file_if_missing() {
  local path="$1"
  local content="$2"
  if [[ -f "$path" ]]; then
    return
  fi
  printf '%b' "$content" | $PREFIX tee "$path" >/dev/null
}

write_service() {
  local name="$1"
  local template="$SCRIPT_DIR/templates/$name.service"
  local target="/etc/systemd/system/$name.service"
  [[ -f "$template" ]] || fail "missing service template: $template"
  local content
  content="$(<"$template")"
  content="${content//__SECLAB_HOME__/${SECLAB_HOME}}"
  printf '%s\n' "$content" | $PREFIX tee "$target" >/dev/null
  $PREFIX chmod 0644 "$target"
}

run_seclab_init_runtime_config() {
  if [[ -n "$PREFIX" ]]; then
    $PREFIX env \
      SECLAB_HOME="$SECLAB_HOME" \
      SECLAB_CONFIG_DIR="$SECLAB_CONFIG_DIR" \
      SECLAB_DB_DIR="$SECLAB_DB_DIR" \
      SECLAB_LOG_DIR="$SECLAB_LOG_DIR" \
      SECLAB_AGENT_SOCKET="$SECLAB_AGENT_SOCKET" \
      RUST_LOG=error \
      /usr/local/bin/seclab init-runtime-config --host "$SECLAB_HOST" --port "$SECLAB_PORT" --public-host "$SECLAB_PUBLIC_HOST" >/dev/null
  else
    SECLAB_HOME="$SECLAB_HOME" \
      SECLAB_CONFIG_DIR="$SECLAB_CONFIG_DIR" \
      SECLAB_DB_DIR="$SECLAB_DB_DIR" \
      SECLAB_LOG_DIR="$SECLAB_LOG_DIR" \
      SECLAB_AGENT_SOCKET="$SECLAB_AGENT_SOCKET" \
      RUST_LOG=error \
      /usr/local/bin/seclab init-runtime-config --host "$SECLAB_HOST" --port "$SECLAB_PORT" --public-host "$SECLAB_PUBLIC_HOST" >/dev/null
  fi
}

log "prepare directories under ${SECLAB_HOME}"
$PREFIX mkdir -p "$SECLAB_CONFIG_DIR" "$SECLAB_DB_DIR" "$SECLAB_LOG_DIR" "$SECLAB_RUN_DIR"

log "write bootstrap security file: ${SECLAB_CONFIG_DIR}/bootstrap-security.json"
BOOTSTRAP_SECURITY_JSON="$(
  printf '{"username":"%s","password":"%s","safe_entry":"%s","password_complexity":false}\n' \
    "$(json_escape "$ADMIN_USERNAME")" \
    "$(json_escape "$ADMIN_PASSWORD")" \
    "$(json_escape "$SAFE_ENTRY")"
)"
printf '%s' "$BOOTSTRAP_SECURITY_JSON" | $PREFIX tee "$SECLAB_CONFIG_DIR/bootstrap-security.json" >/dev/null
$PREFIX chmod 0600 "$SECLAB_CONFIG_DIR/bootstrap-security.json"

log_section "== install agent"
log "install binary: /usr/local/bin/seclab-agent"
install_from_tarball "agent" "seclab-agent"

log "write config: ${SECLAB_CONFIG_DIR}/agent.toml"
write_file_if_missing "$SECLAB_CONFIG_DIR/agent.toml" "# seclab agent config\n"
log "write install dir marker: ${SECLAB_CONFIG_DIR}/agent.install_dir"
write_file_if_missing "$SECLAB_CONFIG_DIR/agent.install_dir" "$SECLAB_HOME"
log "write node role: ${SECLAB_CONFIG_DIR}/node.role"
printf '%s\n' "all" | $PREFIX tee "$SECLAB_CONFIG_DIR/node.role" >/dev/null

log "write service: /etc/systemd/system/seclab-agent.service"
write_service "seclab-agent"
log "agent service prepared; it will start after SecLab is ready"

log_section "== install seclab"
log "install binary: /usr/local/bin/seclab"
install_from_tarball "controller" "seclab"
if [[ -x "$SCRIPT_DIR/slctl" ]]; then
  log "install tool: /usr/local/bin/slctl"
  $PREFIX install -m 0755 "$SCRIPT_DIR/slctl" /usr/local/bin/slctl
  $PREFIX ln -sf /usr/local/bin/slctl /usr/bin/slctl
fi

if command -v openssl >/dev/null 2>&1; then
  JWT_SECRET="$(openssl rand -hex 24)"
else
  JWT_SECRET="seclab_dev_jwt_secret"
fi

log "write config: ${SECLAB_CONFIG_DIR}/seclab.toml"
write_file_if_missing "$SECLAB_CONFIG_DIR/seclab.toml" "jwtSecret = \"$JWT_SECRET\"\nagentBinary = \"/usr/local/bin/seclab-agent\"\nslctlPath = \"/usr/local/bin/slctl\"\n"
log "init SecLab listen config: ${SECLAB_HOST}:${SECLAB_PORT}"
log "default controller callback URL: https://${SECLAB_PUBLIC_HOST}:${SECLAB_PORT}"
run_seclab_init_runtime_config

log "write service: /etc/systemd/system/seclab.service"
write_service "seclab"
log "start service: seclab"
$PREFIX systemctl daemon-reload
$PREFIX systemctl enable --now seclab >/dev/null 2>&1
wait_for_seclab_listener "$SECLAB_PORT"
log "seclab service started"

log "start service: seclab-agent"
$PREFIX systemctl enable --now seclab-agent >/dev/null 2>&1
wait_for_agent_socket "$SECLAB_AGENT_SOCKET"
log "agent service started"
restore_running_suite_socket_containers

log_section "== install completed"
echo "SecLab initial login information:"
echo
echo "  Username: ${ADMIN_USERNAME}"
echo "  Password: ${ADMIN_PASSWORD}"
echo "  URL     : https://${SECLAB_PUBLIC_HOST}:${SECLAB_PORT}/${SAFE_ENTRY}"
echo
echo "Please save the password now. slctl can reset the password but cannot display it later."
