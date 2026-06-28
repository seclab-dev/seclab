# SecLab 架构安全改进规划 (TODO)

本项目旨在实现 SecLab 主控与分布式 Agent 节点之间更坚固的安全边界。以下是根据当前架构设计的安全改进路线图。

---

## 🟥 短期改进 (优先级：高 / 成本：低)

### 1. SSH 凭据“用后即焚”机制

* [ ] **重构部署流程**：将新增部署、预检、升级时的 SSH 凭证限制在前端临时输入和内存传递中，取消在数据库中的持久化落盘存储。
* [ ] **凭证清理**：节点 mTLS 隧道建立并激活后，主控立即从数据库中抹除该节点的 `ssh_password_ciphertext` 与 `ssh_private_key_ciphertext` 字段。
* [ ] **代码实现位置**：
  * 关联文件：`crates/seclab/src/services/node_deploy.rs`
  * 关联文件：`crates/seclab/src/services/node_runtime.rs`

---

## 🟨 中期改进 (优先级：中 / 成本：中)

### 2. 动态 mTLS 证书吊销校验 (CRL)

* [ ] **黑名单校验设计**：在主控端的自定义 `Acceptor` / TLS 握手阶段中，加入证书序列号 (SerialNumber) 或指纹 (Fingerprint) 的动态黑名单匹配逻辑。
* [ ] **吊销联动**：在注销或删除节点时，将其证书加入吊销列表，并在 TLS 握手阶段直接切断其物理连接。
* [ ] **代码实现位置**：
  * 关联文件：`crates/seclab/src/acceptor.rs`
  * 关联文件：`crates/seclab/src/services/node_runtime.rs`

---

## 🟦 长期改进 (优先级：中 / 成本：高)

### 3. Agent 执行权限特权最小化 (Linux Capabilities)

* [ ] **降权部署**：重构 `install.sh` 脚本，避免直接以全局 `root` 权限运行 Agent 进程。
* [ ] **能力分配**：通过 Systemd Service 使用 Linux Capabilities 精细分配特权：
  * 仅赋予 `CAP_NET_RAW` 与 `CAP_NET_ADMIN`（用于网卡流量抓包）。
  * 使用普通的 `seclab` 系统账户身份运行主程序。
* [ ] **代码实现位置**：
  * 关联文件：`deploy/install.sh`

### 4. 强结构化指令白名单过滤

* [ ] **安全 API 约束**：在 Agent 端废除“执行任意 Shell 命令行”类通用控制 API。
* [ ] **强结构化参数校验**：所有下发指令（如启动/停止仿真）改用严格的结构化白名单及边界安全校验，防范主控一旦沦陷导致的全网失控。
* [ ] **代码实现位置**：
  * 关联文件：`crates/seclab-agent/src/`
