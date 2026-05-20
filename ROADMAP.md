# RustProxy 功能路线图

与 HAProxy / Nginx 对标时，以下只列仍需要实现或明显增强的能力。已实现的基础反向代理、Host/Path/Header/Cookie/JWT 匹配、AND/OR 条件树、匹配集、多上游权重、加权轮询、least connections、IP/URL/consistent hash、TCP/HTTP 健康检查、HTTPS/SNI、请求/响应头改写、路径 strip/rewrite/redirect、基础限流、失败重试、WebSocket、Prometheus 指标、access log、配置热更新和管理 UI 不再作为待办列入。

---

## P0 — 生产核心能力

### 1. L4 / TCP 代理与 TLS 透传

当前主要是 HTTP/HTTPS L7 代理。需要补齐：

- TCP 透传代理，支持数据库、Redis、SSH、自定义 TCP 协议。
- TLS passthrough，不解密直接转发。
- 基于 SNI 的 TLS 透传路由。
- TCP 层健康检查、连接数、超时和日志指标。
- 后续再评估 UDP 代理支持。

### 2. 会话保持与 Stick Table

当前只有 hash 类负载均衡，还没有 HAProxy 风格的粘滞状态表。

- Cookie persistence，代理注入或识别后端 cookie。
- 基于 IP、Header、Cookie、JWT claim 的 sticky session。
- Stick table：按 key 记录连接数、请求速率、错误率、字节数等状态。
- Stick table 与限流、ACL、后端选择联动。
- 多实例状态同步，类似 HAProxy peers。

### 3. Runtime API / 运维控制面

当前主要依赖 DB 配置轮询和 Web UI，缺少低延迟运维接口。

- 在线 enable / disable / drain 后端 target。
- 在线调整 target 权重。
- 查看 upstream、target、健康状态、连接数、队列、错误统计。
- 查看和清理 stick table、连接、会话。
- 配置校验、预览 diff、提交失败回滚。
- 可选 Unix socket / local admin API，避免只依赖 Web UI。

### 4. 超时、连接与队列控制细化

当前超时粒度偏粗，需要拆分到生产代理常见维度。

| 能力 | 用途 |
|------|------|
| client_timeout | 客户端读写超时 |
| server_timeout | 上游读写超时 |
| http_request_timeout | 请求头/请求体接收超时 |
| http_keepalive_timeout | 客户端 keep-alive 空闲超时 |
| tunnel_timeout | WebSocket / TCP tunnel 空闲超时 |
| queue_timeout | 无可用连接或达到并发上限时的等待超时 |
| maxconn | 全局、监听、规则、上游、target 级连接上限 |

### 5. 优雅关闭与零停机 Reload

当前已有代理监听热更新，但还需要更严格的生产语义。

- 进程收到 SIGTERM/SIGINT 时停止接收新连接，等待在途请求完成。
- listener 替换时旧 listener drain，而不是短时间后直接 abort。
- 配置 reload 原子化：校验、启动新监听、切流、旧监听 drain、失败回滚。
- Reload 期间保留连接池、健康状态、stick table 等运行时状态。

---

## P1 — 高可用与安全增强

### 6. TLS 增强

- mTLS / 客户端证书校验。
- TLS 版本、密码套件、曲线、ALPN 策略配置。
- 前端 HTTP/2 明确配置与指标。
- OCSP Stapling。
- 证书热加载、证书过期告警、证书使用状态展示。

### 7. 高级健康检查与服务发现

- HTTP check 支持自定义 method、headers、body、期望响应 body/header。
- 多步骤 health check。
- rise/fall、slow start、backup server、drain state。
- agent check / external check。
- DNS resolver：动态解析域名 target，TTL 驱动刷新。
- 服务状态持久化，重启后避免全部 target 从未知状态开始。

### 8. ACL 与规则动作系统增强

当前匹配和动作足够覆盖基础路由，但还不具备 HAProxy 的泛化能力。

- IP 源地址/网段 ACL，支持 allow/deny。
- Method、Query、Scheme、Port、Body size、TLS 信息等匹配条件。
- Map 文件或命名映射表，用于大规模 host/path/header 映射。
- 变量系统：请求生命周期内 set/get 变量。
- 条件动作：按 ACL 执行 deny、redirect、set-header、use-backend、rate-limit。

### 9. 观测与日志增强

- Access log 格式可配置。
- 支持 JSON log、syslog、文件轮转。
- 捕获请求/响应 header、cookie、TLS 信息。
- 分阶段耗时：排队、连接、上游响应、总耗时。
- per-upstream / per-target 指标：健康、连接数、失败数、重试数、队列长度。
- Runtime 状态页或 stats endpoint。

### 10. 管理后台权限与审计

- 角色权限：只读、运维、管理员。
- API token / service account。
- 管理 API IP 白名单。
- 配置变更审计日志。
- 操作确认和回滚入口。

---

## P2 — 高级流量处理

### 11. 压缩

- gzip / brotli。
- 按 content-type、大小、状态码启用。
- 避免重复压缩已压缩响应。

### 12. 缓存

- 静态资源或 API 响应缓存。
- 缓存 key、TTL、bypass、purge。
- 基于 header / status / method 的缓存策略。

### 13. 自定义错误页

- 404 / 429 / 502 / 503 / 504 等错误页可配置。
- 按监听、规则、上游设置错误页。
- 支持 errorfile / error redirect。

### 14. 协议与扩展能力

- HTTP/3 / QUIC 评估。
- PROXY protocol v1/v2 收发。
- gRPC 友好支持与指标。
- WASM / Lua / 插件式扩展评估。
