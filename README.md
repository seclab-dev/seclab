# SecLab

[![Project Status: Active](https://img.shields.io/badge/Project%20Status-Active-emerald.svg)](https://github.com/seclab-dev/seclab)
[![Backend: Rust](https://img.shields.io/badge/Backend-Rust%202024-orange.svg)](https://github.com/seclab-dev/seclab/tree/main/crates)
[![Frontend: Vue 3](https://img.shields.io/badge/Frontend-Vue%203%20%2B%20Vite-blue.svg)](https://github.com/seclab-dev/seclab/tree/main/frontend)

SecLab 是面向安全测试、套件化安全工具、漏洞验证和主机运维的分布式安全实验室平台。系统由主控 `seclab` 和节点执行面 `seclab-agent` 组成，主控负责控制面、Web 控制台和全局状态，Agent 负责本地 Docker、文件系统、进程、网络和任务执行。

![Preview](https://raw.githubusercontent.com/seclab-dev/seclab/main/docs/assets/preview.png)

## 核心能力

- 桌面式 Web 控制台：应用库、多窗口、桌面图标、通知、日志和主题切换。
- 节点管理：本地节点和远程节点纳管，支持健康检查、资源指标和 mTLS 通信。
- Docker 管理：容器、镜像、数据卷、网络和 Compose 项目管理。
- 套件中心：导入、安装、启用、停用和卸载 `.slsp` Compose 套件。
- 主机运维：文件、进程、磁盘、防火墙、终端、脚本和计划任务。
- 在线升级与审计：升级流程、平台事件、运行日志和关键操作审计。

## 目录结构

```text
.
├── crates/
│   ├── seclab/            # 主控服务
│   ├── seclab-agent/      # 节点执行面
│   ├── seclab-api/        # API 响应与错误模型
│   ├── seclab-contracts/  # 前后端契约类型
│   ├── seclab-runtime/    # 运行时抽象
│   ├── seclab-scenario/   # 场景领域模型
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

## 文档

- 文档库：[seclab-docs](https://github.com/seclab-dev/seclab-docs)
