# Agent 统一运行时架构

## 设计目标

本地节点和节点使用相同的身份、会话、任务与事件语义。部署拓扑只影响 Master 调用 Agent 时使用的命令端点：

- 本地节点使用 Unix Domain Socket。
- 节点使用 mTLS HTTPS。

业务模块只能依赖统一运行时上下文，不得根据 Agent mode 分叉业务流程。

## 节点会话与命令授权

所有 Agent 都建立 runtime session。本地节点使用固定节点 ID `local`，节点通过 enrollment 获得节点 ID。

Agent 注册时生成随机 command credential。Master 加密保存凭据，Agent 只保存摘要；`NodeRuntimeClient` 根据活动会话解析 UDS 或 HTTPS 端点并自动携带凭据。Socket 或 HTTPS 探测只记录可达性，节点在线状态由活动会话和 lease 决定。

operation、scheduled-task 和 script-run 使用 Agent durable outbox 主动上报。Master 不再轮询本地 Agent 的事件接口；断线期间事件保留在 Agent，重新注册后继续幂等投递。

## 套件运行时授权

浏览器 `@seclab-dev/suite-sdk` 只处理 iframe 的主题、语言、通知和导航，不参与 Agent 调用。

套件后端需要 Agent 能力时，在清单 `runtime.agent` 中声明服务和最小能力。安装器为实例生成：

- 只读挂载的 `runtime.json`。
- 独立访问令牌。
- 本地 UDS 或节点 HTTPS/TLS 端点。
- 仅作用于声明服务的 Compose override。

套件身份由令牌确定，请求体不能自行声明 suite ID 或 instance ID。工作负载和抓包 API 自动限定到当前实例；`workloads.manage` 与 `captures.manage` 分别控制容器和抓包能力。停用或卸载套件时撤销令牌。

## 传输与业务边界

- endpoint adapter 负责 UDS/HTTPS 客户端构造。
- runtime session 负责身份、在线状态、lease 和命令凭据。
- Agent runtime context 向终端、Docker 和套件暴露稳定运行时事实。
- Compose `ImageResolver` 根据基础设施策略选择本机镜像或 Master 协调获取。
- 业务模块不得读取 Agent mode、Socket 路径或节点监听地址。

## 升级边界

当前为开发阶段，Master、Agent 和套件交付包原子升级。不支持旧 Agent/Master 混合部署、旧套件 Agent 环境变量或历史运行时数据迁移。
