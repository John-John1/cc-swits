# Claude App 接管设计

## 目标

在 `CC Switch` 顶部应用切换区新增一个 `Claude App` 入口。

- 页面 UI 复用现有 `Claude` provider 列表与编辑体验
- provider 数据优先复用 `claude` 现有 schema 与数据库
- `启用` 按钮不再走 `~/.claude/settings.json` 的 CLI takeover
- 改为走“兼容官方 Claude App 的 Remote Control / bridge 机制”
- 官方 `Claude App` 本体不修改，`free-code-main` 仅用于理解机制

## 结论先说

这件事能做，但正确做法不是“让 `cc_switch` 自己托管一个普通 `claude remote-control` 命令，然后把它当成官方客户端接管”。

正确做法是：

1. 用 `free-code-main` 梳理官方客户端依赖的 bridge/session 机制
2. 在 `cc_switch` 中新增一个 `Claude App` 页面
3. 页面仍然操作 `claude` provider 列表
4. 但启用动作切换到一条新的“官方客户端兼容接管”分支
5. 这条分支负责为官方 `Claude App` 提供它期望的本地会话入口，并把底层推理转发到 `Codex Auto / Copilot / 其他 provider`

## free-code-main 机制梳理

### 1. 官方侧不是直接读 CLI 的 `settings.json`

`Claude Code CLI` 的 takeover 主要靠本地配置重写。

官方 `Claude App` 用的是 Remote Control / bridge 机制，源码里的关键入口在：

- `E:\claude_code_full\free-code-main\src\hooks\useReplBridge.tsx`
- `E:\claude_code_full\free-code-main\src\bridge\initReplBridge.ts`
- `E:\claude_code_full\free-code-main\src\bridge\replBridge.ts`
- `E:\claude_code_full\free-code-main\src\bridge\remoteBridgeCore.ts`

### 2. 存在两套桥接形态

#### v1: environment-based bridge

通过 Environments API 建环境，再轮询 work：

- `POST /v1/environments/bridge`
- `GET /v1/environments/{environmentId}/work/poll`
- `POST /v1/environments/{environmentId}/work/{workId}/ack`
- `POST /v1/sessions`

对应源码：

- `E:\claude_code_full\free-code-main\src\bridge\bridgeApi.ts`
- `E:\claude_code_full\free-code-main\src\bridge\createSession.ts`
- `E:\claude_code_full\free-code-main\src\bridge\types.ts`

这个模式下，连接入口会带 `environmentId`：

- 连接 URL: `https://claude.ai/code?bridge={environmentId}`
- 会话 URL: `https://claude.ai/code/{sessionId}?bridge={environmentId}`

见：

- `E:\claude_code_full\free-code-main\src\bridge\bridgeStatusUtil.ts`
- `E:\claude_code_full\free-code-main\src\constants\product.ts`

#### v2: env-less code-session bridge

不再走环境轮询，而是直接创建 code session，再换 worker JWT：

- `POST /v1/code/sessions`
- `POST /v1/code/sessions/{sessionId}/bridge`

`/bridge` 返回：

- `worker_jwt`
- `api_base_url`
- `expires_in`
- `worker_epoch`

对应源码：

- `E:\claude_code_full\free-code-main\src\bridge\codeSessionApi.ts`
- `E:\claude_code_full\free-code-main\src\bridge\remoteBridgeCore.ts`

### 3. bridge 最终仍然会把请求送到 Claude Code 内核

无论 v1/v2，桥接层的目标都是把会话交给 Claude Code 本地内核。

源码里真正拉起本地子进程的是：

- `E:\claude_code_full\free-code-main\src\bridge\sessionRunner.ts`

这里有几个关键信号：

- 子进程运行在 `CLAUDE_CODE_ENVIRONMENT_KIND=bridge`
- v1 走 session-ingress
- v2 走 `/v1/code/sessions/{id}` + worker API
- provider 实际仍然由 Claude Code 本地 API client 决定

也就是说，官方 app 不是自己直接发模型请求，它依赖 bridge 对接到本地 Claude Code 执行端。

### 4. “官方客户端兼容接管”的本质

`cc_switch` 不可能修改官方 app 本体，所以它只能兼容官方 app 所依赖的本地 bridge/session 机制。

目标不是篡改官方界面，而是让官方 app 连上一个由 `cc_switch` 接管过底层 provider 的本地执行端。

## 对 cc_switch 的影响

### 1. 现有 Claude CLI takeover 仍然保留

当前 `cc_switch` 的 Claude takeover 是配置接管型：

- 改写 `~/.claude/settings.json`
- 把 `ANTHROPIC_BASE_URL` 指向本地代理
- 用本地代理把 Anthropic 请求转到其他 provider

核心代码在：

- `E:\cc_switch\source\src-tauri\src\services\proxy.rs`

这条逻辑对 CLI 有效，但不足以单独接管官方 `Claude App`。

### 2. 可以直接复用的层

以下层基本都能继续复用：

- Claude provider 数据本身
- Claude provider 表单与列表 UI
- 现有 provider 数据库表与 schema
- 现有本地代理与请求转换链路
- `claude` 请求到其他 provider 的格式转换
- `claude` / `codex_auto` / `github_copilot` 的认证与 token 注入

可复用代码重点：

- `E:\cc_switch\source\src\components\providers\ProviderList.tsx`
- `E:\cc_switch\source\src\hooks\useProviderActions.ts`
- `E:\cc_switch\source\src-tauri\src\proxy\handlers.rs`
- `E:\cc_switch\source\src-tauri\src\proxy\provider_router.rs`
- `E:\cc_switch\source\src-tauri\src\services\proxy.rs`

### 3. 已有 claude_app 改动里，哪些可留，哪些要改

#### 可留

这些改动方向是对的：

- `settings.rs` 中新增 `visible_apps.claude_app`
- `settings.rs` 中新增 `current_provider_claude_app`
- `provider_router.rs` / `handler_context.rs` 对“provider 来源 app”和“当前选中 key”做了解耦
- `handlers.rs` 中新增 `/claude-app/v1/messages` 的独立入口
- `server.rs` 中新增 `claude-app` 路由

这些属于“新页面与旧页面共用 provider 数据，但拥有独立当前选中 provider”的基础设施。

#### 需要重构

以下改动目前代表的是一条偏掉的实现路线：

- `E:\cc_switch\source\src-tauri\src\services\claude_app.rs`
- `E:\cc_switch\source\src-tauri\src\commands\claude_app.rs`
- `E:\cc_switch\source\src-tauri\src\store.rs` 中的 `claude_app_service`

原因是它们的语义是：

- `cc_switch` 主动 `spawn` 一个 `claude remote-control --spawn=same-dir`
- 再把 `ANTHROPIC_BASE_URL` 指到 `/claude-app/v1/messages`

这更接近“`cc_switch` 自己做一个桥接宿主”，而不是“兼容官方客户端依赖的桥接机制”。

它可以作为实验原型，但不是最终架构。

## 新页面的正确设计

## 前端

### 新入口

在顶部 `AppSwitcher` 新增一个 `Claude App` 图标，位置放在 `Claude` 与 `Codex` 之间。

涉及：

- `E:\cc_switch\source\src\components\AppSwitcher.tsx`
- `E:\cc_switch\source\src\App.tsx`
- `E:\cc_switch\source\src\lib\api\types.ts`
- `E:\cc_switch\source\src\types.ts`

### 页面内容

页面复用现有 Claude provider 列表，不重新发明一套管理界面：

- 同样显示 `GitHub Copilot`、`Codex Auto`、其他 Claude provider
- 同样支持新增、编辑、复制、测试、排序
- 但“当前启用”的状态来源改为 `current_provider_claude_app`

### 前端语义

- `Claude` 页面
  - 代表 `Claude CLI takeover`
- `Claude App` 页面
  - 代表 `Claude App takeover`

两页 UI 很像，但按钮背后的启用动作不同。

## 后端

### Provider 维度

新增页面不应该拥有独立 provider 仓库。

建议：

- provider source 仍然用 `claude`
- 仅“当前选中的 provider key”分离为 `claude_app`
- proxy 配置仍然优先复用 `claude`

也就是：

- provider source: `claude`
- current provider key: `claude_app`
- route key: `claude_app`
- proxy config key: `claude`

这样做的好处：

- 不复制 provider 数据
- 新页面天然复用 Codex Auto / Copilot / key provider
- 仍然允许 CLI 和 App 分别选不同的当前 provider

### Takeover 分层

新页面的启用按钮不要调用旧的：

- `set_takeover_for_app("claude", true)`

而是调用新的 `Claude App compatibility takeover` 服务。

这条服务职责应该是：

1. 启动或维护“官方客户端可连接”的本地 bridge 会话入口
2. 让该入口底层使用 `claude_app` 当前 provider
3. 把模型请求统一打进 `cc_switch` 的 `/claude-app/v1/messages`
4. 维护 bridge/session 状态给前端展示
5. 必要时给出 connect/session URL、状态、错误信息

### 推荐的服务边界

不要继续把它叫做单纯的 `ClaudeAppBridgeService` 并直接 `spawn claude remote-control`。

建议拆成两层：

#### 1. `ClaudeAppTakeoverService`

面向前端按钮，负责：

- 启用/停用 Claude App takeover
- 记录 `claude_app` 当前 provider
- 暴露状态给前端

#### 2. `ClaudeAppCompatSessionService`

面向官方客户端兼容逻辑，负责：

- 管理 bridge/session 生命周期
- 管理 connect URL / session URL
- 处理需要的 worker/session 凭证刷新
- 把底层请求导到 `cc_switch` proxy

最终前端只和 `ClaudeAppTakeoverService` 打交道。

## 建议的最小实现路径

### Phase 1: 页面与路由分离

先完成这些低风险部分：

- 前端新增 `Claude App` 图标
- 页面复用 Claude provider 列表
- `current_provider_claude_app` 生效
- `/claude-app/v1/messages` 路由继续保留

这一步先把“选择谁来接管 app”这件事跑通。

### Phase 2: 兼容接管服务

实现新的 takeover service，但先不要写死为“spawn 一个独立 remote-control 命令”。

先把状态接口做出来：

- 当前 provider
- 兼容接管是否启用
- 当前 session 状态
- connect URL / session URL
- 最近错误

### Phase 3: 对齐官方客户端 bridge/session 行为

对照 `free-code-main` 决定最终兼容的是哪条桥接模式：

- 优先考虑 env-less `code-session / bridge`
- 因为它更接近当前官方的新机制，链路更短

如果落 env-less：

- 需要兼容 `POST /v1/code/sessions`
- 需要兼容 `POST /v1/code/sessions/{id}/bridge`
- 需要管理 worker JWT / epoch 刷新
- 需要维持对官方 app 可用的 session URL 状态

如果必须退回 env-based：

- 需要兼容 environment register / poll / ack / heartbeat
- 复杂度更高，但可作为 fallback

## 风险与边界

### 1. 官方 app 账号体系不能被完全替换

这套方案接管的是底层 provider，不是把官方 `Claude App` 的登录体系整体替掉。

也就是说，大概率仍然需要官方 app 自身完成 Claude 账号登录，才能使用 Remote Control。

### 2. 不能只看 CLI takeover 经验

CLI takeover 的核心是本地配置重写。

App takeover 的核心是官方客户端兼容的 bridge/session 接入。

两者 UI 可以相似，但后端本质不同。

### 3. 当前仓库已有半成品需要收口

仓库里已经有一批 `claude_app` 相关改动，但它们混合了：

- 正确的“页面与 provider key 分离”
- 偏掉的“直接托管 `claude remote-control` 子进程”

继续开发前，建议先按本设计把这两类改动拆开，避免后面边修边绕。

## 当前建议

下一步按下面顺序推进最稳：

1. 保留 `settings.rs` / `provider_router.rs` / `handler_context.rs` / `handlers.rs` / `server.rs` 里的分离基础设施
2. 暂停继续扩展当前 `services/claude_app.rs`
3. 先把前端 `Claude App` 页面做出来，并接上独立 current-provider
4. 再单独实现真正的 `Claude App compatibility takeover` 服务
5. 最后再决定这条服务内部走 env-less 还是 env-based 的兼容实现

## 一句话总结

`Claude App` 新页面可以和 `Claude` 页面长得几乎一样，但启用按钮背后必须从“改 CLI live 配置”切换到“兼容官方客户端桥接机制”的新分支；`free-code-main` 证明这条路架构上成立，而 `cc_switch` 现有代理与 provider 体系足够承担底层转发。
