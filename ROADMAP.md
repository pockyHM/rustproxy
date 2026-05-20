# RustProxy 功能路线图

本路线图用于和 HAProxy / Nginx 的生产能力对标。已落地的能力不再作为待实现项列入 P0；P0 只保留发布前必须验证或补齐的收尾工作。

当前已实现并具备自动化测试覆盖的核心能力包括：

- 基础 HTTP/HTTPS 反向代理、Host/Path/Header/Cookie/JWT 匹配、AND/OR 条件树、匹配集。
- 多上游权重、加权轮询、least connections、IP hash、URL hash、consistent hash。
- HTTP/TCP 健康检查、失败重试、WebSocket。
- HTTPS 监听、证书上传、SNI 证书选择。
- TCP 透传、TLS passthrough、基于 SNI 的 TLS 透传路由。
- 请求/响应 Header 策略、路径 strip/rewrite/redirect。
- 基础限流、连接上限、队列超时。
- 全局与路由级超时策略；upstream/target 不再承载超时配置。
- Sticky session、Cookie persistence、基础 stick table。
- Runtime API 与 Admin UI 支持 target enable/disable/drain、权重覆盖、运行时 upstream/target 状态查看。
- Prometheus 指标、TCP 指标、access log、配置热更新、监听热替换失败回滚。
- Admin UI 覆盖规则、匹配集、上游、TCP target、证书、全局配置、TCP listener、运行时目标控制。

---

## P0 — 发布前验收与稳定化

### 1. Admin UI 验收

- 验证 upstream 目标可配置 `http://`、`https://`、`tcp://`，并能被 HTTP route 或 TCP listener 正确引用。
- 验证 route 表单的高级配置折叠区能正确保存超时策略、Header 策略、路径动作和限流策略。
- 验证 Config File 页面中的 TCP listener、SNI routes、全局超时、连接上限保存后能热更新。
- 验证旧配置中 upstream/target 的 `timeouts` 字段会被忽略，不再从 UI 生成。

### 2. 真实链路 E2E

- 使用 Redis/MySQL/SSH 或自定义 TCP echo 服务验证 TCP 透传。
- 使用真实 TLS ClientHello / SNI 验证 TLS passthrough 路由和默认 upstream fallback。
- 使用真实 WebSocket 长连接验证 tunnel 转发和 tunnel timeout 行为。
- 验证 route timeout 覆盖 global timeout，global timeout 作为默认值生效。

### 3. Runtime 运维控制验证

- 在持续流量下验证 target enable/disable/drain 的选择行为和 active connections 变化。
- 验证运行时权重覆盖不会写回配置，reload 后符合预期。
- 验证 `/api/runtime/upstreams`、`/api/runtime/stick-table` 与 Admin UI 展示一致。

### 4. Reload / Drain / Rollback 验收

- 修改 HTTP、HTTPS、TCP listener 后验证监听热替换不断流。
- 在长请求或长连接期间触发 reload / shutdown，验证旧 listener drain 行为。
- 故意提交端口冲突、无效 TCP listener、无效证书配置，验证失败回滚到旧配置。

### 5. 文档与配置样例同步

- README 和示例配置更新为全局 + route 两级超时策略。
- 增加 TCP upstream、TCP listener、TLS passthrough、sticky、runtime target 操作示例。
- 增加 P0 手工验收清单，记录真实环境验证结果。

---

## P1 — 生产增强

### 1. Runtime API / 运维控制面增强

- 清理 stick table 指定 key、指定 upstream 或全部条目。
- 清理或踢出连接、会话。
- 配置预览 diff、提交确认、失败原因展示、手动回滚入口。
- 可选 Unix socket / local admin API，降低对 Web UI 的依赖。
- Runtime 操作审计日志。

### 2. Stick Table 增强

- Stick table 记录连接数、请求速率、错误率、字节数等状态。
- Stick table 与限流、ACL、后端选择联动。
- 多实例状态同步，类似 HAProxy peers。
- Prometheus 暴露 stick table 关键指标。

### 3. Reload / Drain 语义增强

- 进程收到 SIGTERM/SIGINT 时停止接收新连接，等待在途请求完成。
- listener 替换时更精细地区分 HTTP 请求、WebSocket、TCP tunnel 的 drain 策略。
- Reload 期间更明确地保留连接池、健康状态、stick table 和 runtime target override。
- 增加 reload/drain 的端到端自动化测试。

### 4. TLS 增强

- mTLS / 客户端证书校验。
- TLS 版本、密码套件、曲线、ALPN 策略配置。
- 前端 HTTP/2 明确配置与指标。
- OCSP Stapling。
- 证书热加载、证书过期告警、证书使用状态展示。

### 5. 高级健康检查与服务发现

- HTTP check 支持自定义 method、headers、body、期望响应 body/header。
- 多步骤 health check。
- rise/fall、slow start、backup server、drain state。
- agent check / external check。
- DNS resolver：动态解析域名 target，TTL 驱动刷新。
- 服务状态持久化，重启后避免全部 target 从未知状态开始。

### 6. ACL 与规则动作系统增强

- IP 源地址/网段 ACL，支持 allow/deny。
- Method、Query、Scheme、Port、Body size、TLS 信息等匹配条件。
- Map 文件或命名映射表，用于大规模 host/path/header 映射。
- 变量系统：请求生命周期内 set/get 变量。
- 条件动作：按 ACL 执行 deny、redirect、set-header、use-backend、rate-limit。

### 7. 观测与日志增强

- Access log 格式可配置。
- 支持 JSON log、syslog、文件轮转。
- 捕获请求/响应 header、cookie、TLS 信息。
- 分阶段耗时：排队、连接、上游响应、总耗时。
- per-upstream / per-target 指标：健康、连接数、失败数、重试数、队列长度。
- Runtime 状态页或 stats endpoint。

### 8. 管理后台权限与审计

- 角色权限：只读、运维、管理员。
- API token / service account。
- 管理 API IP 白名单。
- 配置变更审计日志。
- 高风险操作确认。

---

## P2 — 高级流量处理

### 1. 压缩

- gzip / brotli。
- 按 content-type、大小、状态码启用。
- 避免重复压缩已压缩响应。

### 2. 缓存

- 静态资源或 API 响应缓存。
- 缓存 key、TTL、bypass、purge。
- 基于 header / status / method 的缓存策略。

### 3. 自定义错误页

- 404 / 429 / 502 / 503 / 504 等错误页可配置。
- 按监听、规则、上游设置错误页。
- 支持 errorfile / error redirect。

### 4. 协议与扩展能力

- UDP 代理评估。
- HTTP/3 / QUIC 评估。
- PROXY protocol v1/v2 收发。
- gRPC 友好支持与指标。
- WASM / Lua / 插件式扩展评估。
