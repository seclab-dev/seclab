# SecLab Docker 管理控制台设计文档

## 1. 概述与目标

`DockerManagerView.vue` 是 SecLab 控制台的核心组件之一，旨在提供一个统一的界面来管理和监控底层的 Docker 资源（容器、镜像、网络）。

---

## 2. 架构与布局

### 2.1 结构层次

`DockerManagerView.vue` 充当**智能容器（Smart Container）**，负责数据获取、状态管理、操作处理和组件切换。实际的视图渲染逻辑位于独立的**展示组件（Presentational Components）**中。

| 组件名称 | 角色 | 核心职责 |
| :--- | :--- | :--- |
| **`DockerManagerView.vue`** (父组件) | 布局与控制器 | 菜单渲染、状态管理 (`ref`s)、API 调用 (`fetch*`)、操作派发 (`handle*`)、工具函数 (`utils`) 定义、动态组件切换。 |
| **`DockerOverview.vue`** | 子组件/展示 | 渲染 Docker 概览和系统指标。 |
| **`DockerProjectContainers.vue`** | 子组件/展示 | 渲染 Compose/项目容器列表。 |
| **`DockerAllContainers.vue`** | 子组件/展示 | 渲染所有容器列表。 |
| **`DockerImages.vue`** | 子组件/展示 | 渲染镜像列表。 |
| **`DockerNetworks.vue`** | 子组件/展示 | 渲染网络列表。 |

### 2.2 核心布局

组件采用左右分栏布局：

* **左侧侧边栏 (`app-docker-sidebar`)**：固定宽度，用于展示概览、项目、容器、镜像、卷和网络入口，并控制 `activeMenu` 状态。
* **右侧内容区 (`app-docker-content-area`)**：动态区域，用于显示当前 `activeMenu` 对应的子组件（通过 `<component :is="activeComponent" />` 渲染）。

Docker 状态变更的审计记录统一由全局“操作日志”应用提供，Docker 应用不重复设置日志入口。

---

## 3. 数据流与状态管理

### 3.1 核心状态 (`ref`s)

| 状态名称 | 类型 | 用途 |
| :--- | :--- | :--- |
| `activeMenu` | `ref<string>` | 当前激活的菜单键。 |
| `isLoading` | `ref<boolean>` | 全局加载状态，用于在数据获取期间显示加载指示器。 |
| `containers` / `projectContainers` / `imagesList` / `networks` | `ref<Array>` | 各个视图的原始数据列表。 |
| `dockerStatus` / `totalContainerCount` 等 | `ref<number\|boolean>` | 概览视图所需的各项指标。 |

### 3.2 数据生命周期与按需刷新 (Lazy & Fresh Loading)

数据加载逻辑通过顶层 `await` 和 `watch` 组合实现，以确保效率和数据时效性。

1. **初始加载 (`initialLoad`)**：
    * 在组件 `setup` 阶段，通过 `await initialLoad()` 阻塞一次，仅请求 **概览视图** 所需的接口 (`fetchOverviewStatus`)。
    * 目的：快速初始化仪表板指标，避免应用启动时等待所有列表数据。

2. **按需刷新 (`watch(activeMenu)`)**：
    * 监听 `activeMenu` 的变化，并在每次切换到数据视图（`overview`, `containers`, `projects`, `images`, `networks`）时，**无条件**地调用对应的 `fetch*` 函数。
    * 目的：保证用户在切换回来时，看到的数据永远是最新状态。

### 3.3 资源监控缓存

* Agent 侧定时采样 Docker 资源（默认 10 分钟），写入本地缓存表。
* 前端折线图仅读取时间序列（历史缓存）；实时卡片使用概览实时接口。
* 缓存数据默认保留 12 小时，可在 `agent.toml` 中调整。

---

## 4. API 接口集成 (后端职责划分)

### 4.1 核心数据接口 (集中在 `dockerApi`)

> 实际请求路径均以 `/api/v1/agent` 为前缀。

| 接口方法 | 后端 Endpoint | 数据结构 | 用途 |
| :--- | :--- | :--- | :--- |
| `fetchOverviewRealtime` | `/agent/docker/overview/realtime` | `OverviewRealtimeResponse` | 获取 Docker 概览实时数据（状态 + 实时资源 + 运行容器列表）。 |
| `listContainers` | `/agent/docker/containers` | `ContainerSummary[]` | 获取所有容器列表。 |
| `listProjectContainers` | `/agent/docker/compose/containers` | `ContainerSummary[]` | 获取项目/Compose 相关的容器列表。 |
| `listImages` | `/agent/docker/images` | `ImageSummary[]` | 获取所有镜像列表。 |
| `fetchDaemonSettings` | `/agent/docker/daemon/settings` | `DockerDaemonSettings` | 读取当前节点的镜像加速与 daemon 代理。 |
| `updateDaemonSettings` | `/agent/docker/daemon/settings` | `DockerDaemonSettings` | 校验、写入配置并重启当前节点 Docker。 |
| `listNetworks` | `/agent/docker/networks` | `Network[]` | 获取所有 Docker 网络列表。 |
| `fetchSdu` | `/agent/docker/sdu` | `number` | 获取 Docker 逻辑磁盘总占用量。 |
| `fetchResourceUsageHistory` | `/agent/docker/stats/history` | `ResourceUsageHistory` | 获取 Docker 资源趋势（时间序列）。 |
| `fetchContainerResourceUsageSummaries` | `/agent/docker/containers/stats/summary` | `ContainerStatsBatchResponse` | 获取容器资源统计（批量缓存最新值）。 |
| `fetchContainerResourceUsageHistory` | `/agent/docker/containers/{id}/stats/history` | `ResourceUsageHistory` | 获取单容器资源趋势（时间序列）。 |
| `fetchContainerResourceUsageHistoryAll` | `/agent/docker/containers/stats/history` | `ContainerStatsHistoryAllResponse` | 获取所有容器资源趋势（包含容器名称）。 |

### 4.2 WebSocket 实时日志

* **Endpoint**: `ws:///api/v1/agent/websocket/events/ws`
* **用途**: 容器详情通过 `useWebSocketStore` 订阅指定容器的 stdout/stderr 实时日志流。

### 4.2.1 当前约束

* 该链路由 `seclab` 统一暴露入口，再转发到目标 `agent`。
* 它只服务于容器 stdout/stderr 场景，不替代全局操作日志或通知历史查询接口。

### 4.3 操作接口

* **`performAction`**: `POST /agent/docker/action`
  * 用于容器的 `start`, `stop`, `restart`, `remove` 操作；当前实现依赖传入的容器名称执行。

所有操作（如启动、停止、删除）都通过子组件的 `emit` 事件向上派发给父组件 `DockerManagerView.vue` 中的 `handleContainerAction` 或 `handleDeleteImage` 函数进行处理。

| 功能模块 | 核心操作 | 关键流程 |
| :--- | :--- | :--- |
| **容器管理** | 启动/停止/删除 | 1. 触发 `handleContainerAction`。 2. 如果是 `delete`，显示 `ConfirmationModal`。 3. 调用 `dockerApi.performAction`。 4. 成功后，刷新相关列表 (`fetchContainers`/`fetchProjectContainers`/`fetchOverviewData`)。 |
| **镜像管理** | 删除 | 1. 触发 `handleDeleteImage`。 2. 显示 `ConfirmationModal`。 3. 调用 `dockerApi.performAction`。 4. 成功后，刷新镜像列表和概览数据。 |
| **镜像设置** | 镜像加速与代理 | 管理 `/etc/docker/daemon.json` 中的 `registry-mirrors` 和 `proxies`。Agent 保留其他字段，使用 `dockerd --validate` 校验并原子写入；重启失败时恢复原配置和 Docker 服务。配置按当前节点生效。 |

镜像设置中的单一代理地址同时写入 `http-proxy` 和 `https-proxy`，接受 `http://` 或 `https://` URL。代理 URL 可以携带用户名和密码，凭据会明文保存在目标节点 `/etc/docker/daemon.json` 并向已认证管理员回显，因此不得在平台日志中记录配置内容。保存操作会重启目标节点 Docker，运行中的容器可能短暂中断。

---

## 6. 工具函数 (Utils)

所有数据格式化函数统一在 `DockerManagerView.vue` 中定义，并封装在 `utilityFns` 对象中，作为单个 `prop` 传递给子组件。

| 函数名称 | 用途 |
| :--- | :--- |
| `getStateEmoji` | 根据容器状态（`running`, `exited` 等）返回对应的 Emoji 符号。 |
| `formatPorts` | 将端口映射数组格式化为可读的字符串（如 `0.0.0.0:8080->80/tcp`）。 |
| `getContainerIP` | 从容器的网络设置中提取内部 IP 地址。 |
| `formatImageTags` | 格式化镜像的 RepoTags。 |
| `formatBytes` | 将字节数转换为 `KB`, `MB`, `GB` 等可读格式。 |
| `formatIpamConfig` | 格式化网络 IPAM 配置，显示子网和网关信息。 |
| `getConnectedContainerCount` | 计算连接到某个网络的容器数量。 |
