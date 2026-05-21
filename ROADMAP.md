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

## 建议实现顺序

该顺序基于生产可用性、线上变更风险、问题定位能力和实现投入产出综合排序。P1/P2 原始分组仍保留；实际开发时优先按本节选择下一项。

1. **P1 Reload / Drain 语义增强**：最高优先级。优先补齐 SIGTERM/SIGINT graceful shutdown、reload/drain E2E 测试、连接池/健康状态/stick table/runtime target override 在 reload 期间的保留语义。原因是代理系统上线后的发布、重载、长连接处理都依赖这部分稳定性。
2. **P1 Runtime API / 运维控制面增强**：很高优先级。优先做 stick table 清理、配置预览 diff、失败原因展示、手动回滚入口、Runtime 操作审计日志。原因是线上故障需要快速干预，而不是只能改配置后 reload。
3. **P1 观测与日志增强**：很高优先级。优先做 JSON access log、分阶段耗时、per-upstream / per-target 指标、Runtime stats endpoint。原因是没有足够观测能力时，生产问题很难定位。
4. **P1 高级健康检查与服务发现**：高优先级。优先做 HTTP check method/headers/body、期望响应 body/header、rise/fall、slow start、backup server。DNS resolver 和服务状态持久化可以后置。
5. **P1 管理后台权限与审计**：高优先级。优先做 RBAC、API token / service account、配置变更审计、高风险操作确认。多人或生产环境使用时这是安全边界。
6. **P1 TLS 增强**：中高优先级。优先做 mTLS、TLS 版本/ALPN 策略、证书过期告警。OCSP Stapling 和完整证书使用状态展示可以后置。
7. **P1 ACL 与规则动作系统增强**：中高优先级。优先做 IP/CIDR allow-deny、Method/Query/Scheme/Port 匹配、条件动作。变量系统和 map 文件会扩大配置复杂度，建议后置。
8. **P1 Stick Table 增强**：中优先级。优先做 Prometheus 指标、与限流/后端选择联动。多实例 peers 同步复杂度高，建议最后实现。
9. **P2 PROXY protocol v1/v2 收发**：P2 中最高优先级。部署在 LB、Nginx 或云负载均衡后面时，该能力会影响真实源 IP、审计和限流。
10. **P2 自定义错误页**：中优先级。实现成本可控，优先支持 404 / 429 / 502 / 503 / 504，并允许按监听、规则、上游配置。
11. **P2 gRPC 友好支持与指标**：中优先级。适合现代服务代理场景，但需要先明确 HTTP/2 前后端语义和指标口径。
12. **P2 压缩**：中低优先级。gzip / brotli 有价值，但要处理 content-encoding、streaming、重复压缩和 CPU 成本。
13. **P2 缓存**：中低优先级且复杂度高。缓存 key、TTL、bypass、purge 和 header 策略会形成较大子系统，除非目标场景明确，否则不建议太早做。
14. **P2 UDP 代理评估**：低到中优先级。与当前 HTTP/TCP 模型差异较大，按真实需求再评估。
15. **P2 HTTP/3 / QUIC 评估**：低优先级且复杂度高。涉及连接模型、TLS 和运行时链路的较大改动。
16. **P2 WASM / Lua / 插件式扩展评估**：低优先级且复杂度最高。会显著影响安全模型、配置模型和运行时边界，建议最后评估。

推荐近期主线：**Reload/Drain E2E 与 graceful shutdown → Runtime diff/rollback/audit/stick 清理 → 观测增强 → PROXY protocol**。

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
