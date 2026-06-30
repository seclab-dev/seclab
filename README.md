# SecLab

[![Project Status: Active](https://img.shields.io/badge/Project%20Status-Active-emerald.svg)](https://github.com/seclab-dev/seclab)
[![Backend: Rust](https://img.shields.io/badge/Backend-Rust%202024-orange.svg)](https://github.com/seclab-dev/seclab/tree/main/crates)
[![Frontend: Vue 3](https://img.shields.io/badge/Frontend-Vue%203%20%2B%20Vite-blue.svg)](https://github.com/seclab-dev/seclab/tree/main/frontend)

SecLab 是面向安全测试、协议仿真、漏洞验证和主机运维的分布式安全实验室平台。系统由主控 `seclab` 和节点执行面 `seclab-agent` 组成，主控负责控制面、Web 控制台和全局状态，Agent 负责本地 Docker、文件系统、进程、网络和任务执行。

## 核心能力

- 桌面式 Web 控制台：应用库、多窗口、桌面图标、通知、日志和主题切换。
- 节点管理：本地节点和远程节点纳管，支持健康检查、资源指标和 mTLS 通信。
- Docker 管理：容器、镜像、数据卷、网络和 Compose 项目管理。
- 套件中心：导入、安装、启用、停用和卸载 `.slsp` Compose 套件。
- 协议仿真：规则库导入、仿真实例运行和 PCAP 取证。
- 主机运维：文件、进程、磁盘、防火墙、终端、脚本和计划任务。
- 在线升级与审计：升级流程、平台事件、运行日志和关键操作审计。

## 架构

```text
Vue Web Console
       |
       v
seclab 控制面
       |
       +-- Unix Socket --> local seclab-agent
       |
       +-- HTTPS / WSS + mTLS --> remote seclab-agent
                                    |
                                    v
                                  Docker / Host
```

核心命名：

| 名称 | 英文 | 标识 | 说明 |
| --- | --- | --- | --- |
| 主控 | Master | `seclab` | 控制面服务，全局状态来源。 |
| 本地节点 | Local Node | `local` | 主控本机内置工作节点。 |
| 节点 | Node | UUID | 平台纳管的外部计算节点。 |

## 技术栈

- 后端：Rust、Axum、Tokio、SQLx、Tower、Bollard、Rustls。
- 前端：Vue 3、Vite、TypeScript、Pinia、Vue I18n。
- UI：`@seclab-dev/tokens`、`@seclab-dev/icons`、`@seclab-dev/vue`。
- 运行时：Docker、Docker Compose、systemd。

## 目录结构

```text
.
├── crates/
│   ├── seclab/            # 主控服务
│   ├── seclab-agent/      # 节点执行面
│   ├── seclab-api/        # API 响应与错误模型
│   ├── seclab-contracts/  # 前后端契约类型
│   ├── seclab-runtime/    # 运行时抽象
│   ├── seclab-scenario/   # 仿真场景领域
│   ├── seclab-security/   # 证书与安全通信
│   ├── seclab-slctl/      # 命令行工具
│   └── seclab-upgrade/    # 升级能力
├── frontend/              # Vue Web 控制台
├── docs/                  # 当前仓库文档入口
├── scripts/               # 构建、打包和版本脚本
└── Cargo.toml
```

## 开发命令

后端：

```bash
cargo run -p seclab -- start
cargo run -p seclab-agent -- start
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

前端：

```bash
pnpm -C frontend install
pnpm -C frontend dev
pnpm -C frontend lint
pnpm -C frontend build
```

发布包构建：

```bash
./scripts/build-release.sh
```

版本同步：

```bash
pnpm version:set 0.1.0-alpha.1
```

## 文档

- 当前仓库文档入口：[docs/README.md](docs/README.md)
- 组织级文档源：`linked-docs -> ../seclab-docs`
