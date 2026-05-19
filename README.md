# RustProxy

Repository: <http://gitlab.info.dbappsecurity.com.cn/yu.hangjie/rustproxy>

RustProxy is a Rust-based reverse proxy and traffic routing gateway with an embedded admin UI, SQLite-backed configuration, hot-reloaded proxy listeners, reusable match sets, upstream health checks, TLS certificate management, and Prometheus metrics.

---

## 中文

### 功能概览

- 基于规则的反向代理：按监听地址、优先级、创建顺序匹配路由规则。
- 条件匹配：支持 Host、Path、Header、Cookie、JWT Claim。
- 条件组：支持 AND / OR 嵌套条件树。
- 匹配集：可创建可复用的匹配条件集合，并在多个路由规则中引用；也保留规则内直接新建匹配条件。
- 兜底规则：当同一监听入口下普通规则都未命中时，转发到兜底上游。
- 上游池：支持多目标、权重、WebSocket、跳过上游 SSL 校验。
- 健康检查：支持 TCP / HTTP 探测，自动跳过不健康目标。
- HTTP / HTTPS 监听：支持规则级 HTTPS、证书上传、同端口多证书 SNI 选择。
- 端口策略：HTTP 与 HTTPS 监听端口不能冲突，暂不支持同一端口混合 HTTP/HTTPS。
- 证书文件化：证书默认保存到 `/etc/rustproxy/cert.d/<name>/`，配置中只保存绝对路径。
- 配置热更新：代理监听端口支持热更新；管理端监听地址变更仍需重启。
- 管理后台：内置 React UI，提供规则、匹配集、上游、证书、配置文件、运行概览管理。
- 指标观测：暴露 `/metrics`，包含请求、延迟、活跃连接、进程资源等 Prometheus 指标。

### 快速开始

#### 1. 获取代码

```bash
git clone http://gitlab.info.dbappsecurity.com.cn/yu.hangjie/rustproxy.git
cd rustproxy
```

#### 2. 构建管理 UI

管理 UI 会被嵌入到 Rust 二进制中，构建 Rust 前需要先生成 `ui/dist`。

```bash
cd ui
npm install
npm run build
cd ..
```

#### 3. 构建后端

```bash
cargo build --release
```

#### 4. 创建管理员

```bash
./target/release/rustproxy user add admin
```

#### 5. 启动服务

```bash
./target/release/rustproxy serve config.yaml
```

默认地址：

- 管理后台：`http://0.0.0.0:3000/admin/`（本机也可用 `http://127.0.0.1:3000/admin/`）
- Prometheus 指标：`http://0.0.0.0:3000/metrics`
- 反向代理入口：`0.0.0.0:80`

默认 SQLite 数据库路径是 `/var/lib/rustproxy/rustproxy.db`。首次运行前需要确保目录存在且当前用户可写：

```bash
sudo mkdir -p /var/lib/rustproxy
sudo chown "$(id -u):$(id -g)" /var/lib/rustproxy
```

如果需要覆盖数据库路径：

```bash
RUSTPROXY_DB=/path/to/rustproxy.db ./target/release/rustproxy serve config.yaml
```

或：

```bash
./target/release/rustproxy --db /path/to/rustproxy.db serve config.yaml
```

### 配置示例

```yaml
listen: "0.0.0.0:3000"
proxy_listen: "0.0.0.0:80"
connect_timeout: 10
request_timeout: 60
pool_max_idle_per_host: 32
pool_idle_timeout: 90
tcp_keepalive: 60
certificate_dir: "/etc/rustproxy/cert.d"

certificates:
  - name: "example"
    cert: "/etc/rustproxy/cert.d/example/cert.pem"
    key: "/etc/rustproxy/cert.d/example/key.pem"

match_sets:
  - name: "tenant-a"
    conditions:
      type: leaf
      conditionType: header
      key: "x-tenant"
      claimPath: null
      operator: exact
      value: "a"

rules:
  - id: "rule-api"
    name: "API traffic"
    listen: "0.0.0.0:80"
    host:
      type: exact
      value: "api.example.com"
    location:
      type: prefix
      value: "/api"
    priority: 100
    match_set: "tenant-a"
    conditions: null
    upstream: "api"
    weight: 100
    is_fallback: false
    tls: null

  - id: "rule-web-https"
    name: "HTTPS web traffic"
    listen: "0.0.0.0:443"
    host:
      type: exact
      value: "www.example.com"
    location:
      type: prefix
      value: "/"
    priority: 90
    conditions: null
    upstream: "web"
    weight: 100
    is_fallback: false
    tls:
      enabled: true
      certificate: "example"

  - id: "rule-default"
    name: "Default backend"
    listen: "0.0.0.0:80"
    host:
      type: any
      value: null
    location:
      type: prefix
      value: "/"
    priority: 0
    conditions: null
    upstream: "web"
    weight: 100
    is_fallback: true
    tls: null

upstreams:
  api:
    name: "api"
    skip_ssl: false
    websocket: false
    targets:
      - url: "http://127.0.0.1:8081"
        weight: 100
    health_check:
      enabled: true
      mode: http
      path: "/health"
      expected_status: 200
      interval_seconds: 10
      timeout_seconds: 2
      healthy_threshold: 2
      unhealthy_threshold: 2

  web:
    name: "web"
    skip_ssl: false
    websocket: true
    targets:
      - url: "http://127.0.0.1:8080"
        weight: 100
    health_check:
      enabled: false
      mode: tcp
      path: "/health"
      expected_status: 200
      interval_seconds: 10
      timeout_seconds: 2
      healthy_threshold: 2
      unhealthy_threshold: 2

fallback:
  url: "404"
```

### 匹配规则说明

- 请求只会匹配当前监听入口下的规则，例如访问 `0.0.0.0:80` 只匹配 `listen: "0.0.0.0:80"` 的规则。
- 先按 `host` 匹配，优先级为 `exact > wildcard > any`。
- 再按 `location` 匹配，优先级为 `exact > 最长 prefix > regex`。
- 选定最具体的 `host + location` 后，只在这个分组内按 `priority` 从高到低匹配普通规则。
- 如果优先级相同，按创建顺序优先匹配。
- 兜底规则按 `listen + host + location` 生效，只在同一分组普通规则全部未命中后执行；不会自动回退到父 location。
- 规则配置了 `match_set` 时，运行时使用匹配集里的条件；未配置 `match_set` 时，使用规则自身的 `conditions`。
- `match_set` 和规则内条件只允许 header / cookie / jwt，host 和 location 需要在规则上单独配置。
- `listen` 不建议为空；未设置时会归一化为全局 `proxy_listen`，默认是 `0.0.0.0:80`。

### TLS 与证书

- 证书通过管理后台上传，默认保存到 `certificate_dir/<证书名称>/cert.pem` 和 `key.pem`。
- 配置文件中只保存证书文件绝对路径和证书名称。
- 支持 PEM、CRT、CER、DER 等常见证书/私钥输入格式。
- 多个 HTTPS 规则可以监听同一个端口，并通过 SNI / 规则 `host` 选择证书；`host:any` 可作为默认 HTTPS 证书。
- HTTP 与 HTTPS 监听端口不能冲突。

### 常用 CLI

```bash
# 启动
rustproxy serve config.yaml

# 管理用户
rustproxy user add admin
rustproxy user list
rustproxy user passwd admin

# 查看 / 设置全局配置
rustproxy config get
rustproxy config get proxy_listen
rustproxy config set proxy_listen 0.0.0.0:8080

# 上游管理
rustproxy config upstream list
rustproxy config upstream add api http://127.0.0.1:8081 --weight 100
rustproxy config upstream add-target api http://127.0.0.1:8082 --weight 50
rustproxy config upstream delete api

# 规则管理
rustproxy config rule list
rustproxy config rule add rule-api --name "API" --upstream api --priority 100 --listen 0.0.0.0:80 --host-type exact --host api.example.com --location-type prefix --location /api --condition-type header --key x-tenant --operator exact --value a
rustproxy config rule delete rule-api

# 配置导入导出
rustproxy config export
rustproxy config import config.yaml --replace
rustproxy config edit
```

### 开发

```bash
# 前端开发
cd ui
npm install
npm run dev

# 前端生产构建
npm run build

# 后端检查和测试
cd ..
cargo check
cargo test
```

---

## English

### Overview

RustProxy is a reverse proxy and traffic routing gateway written in Rust. It ships with an embedded React admin UI, SQLite-backed configuration, rule-based routing, reusable match sets, TLS listener management, upstream health checks, and Prometheus metrics.

### Features

- Rule-based reverse proxy with listener-aware matching.
- Match conditions for Host, Path, Header, Cookie, and JWT claims.
- Nested AND / OR condition groups.
- Reusable match sets that can be attached to routing rules, while still supporting inline rule conditions.
- Fallback rules for unmatched traffic on the same listener.
- Upstream pools with weighted targets, WebSocket support, and optional upstream SSL verification skipping.
- TCP and HTTP health checks.
- HTTP and HTTPS proxy listeners with hot listener updates.
- Rule-level TLS and certificate upload from the admin UI.
- Certificate files stored on disk, with absolute paths persisted in YAML/SQLite.
- Port conflict protection: HTTP and HTTPS cannot bind the same port at the same time.
- Embedded admin UI for rules, match sets, upstreams, certificates, config, and operations overview.
- Prometheus `/metrics` endpoint for traffic, latency, active connections, and process resource metrics.

### Quick Start

#### 1. Clone

```bash
git clone http://gitlab.info.dbappsecurity.com.cn/yu.hangjie/rustproxy.git
cd rustproxy
```

#### 2. Build the Admin UI

The admin UI is embedded into the Rust binary from `ui/dist`, so build it before compiling the backend.

```bash
cd ui
npm install
npm run build
cd ..
```

#### 3. Build the Backend

```bash
cargo build --release
```

#### 4. Create an Admin User

```bash
./target/release/rustproxy user add admin
```

#### 5. Start RustProxy

```bash
./target/release/rustproxy serve config.yaml
```

Default endpoints:

- Admin UI: `http://0.0.0.0:3000/admin/` (or `http://127.0.0.1:3000/admin/` locally)
- Prometheus metrics: `http://0.0.0.0:3000/metrics`
- Reverse proxy listener: `0.0.0.0:80`

The default SQLite database path is `/var/lib/rustproxy/rustproxy.db`. Before the first run, make sure the directory exists and is writable by the current user:

```bash
sudo mkdir -p /var/lib/rustproxy
sudo chown "$(id -u):$(id -g)" /var/lib/rustproxy
```

Override the database path with:

```bash
RUSTPROXY_DB=/path/to/rustproxy.db ./target/release/rustproxy serve config.yaml
```

or:

```bash
./target/release/rustproxy --db /path/to/rustproxy.db serve config.yaml
```

### Configuration Notes

- `listen` is the admin API and admin UI address.
- `proxy_listen` is the default HTTP reverse proxy listener.
- `certificate_dir` controls where uploaded certificates are stored. The default is `/etc/rustproxy/cert.d`.
- `match_sets` defines reusable header/cookie/JWT condition trees.
- A rule with `match_set` uses that match set at runtime. A rule without `match_set` uses its inline `conditions`.
- Requests are matched only against rules on the current listener.
- Host is matched before rule conditions. Host priority is `exact > wildcard > any`.
- Location is matched after host. Location priority is `exact > longest prefix > regex`.
- Higher `priority` matches first only inside the selected listener + host + location group.
- Equal priorities preserve creation order.
- Fallback rules are evaluated after all normal rules miss in the same listener + host + location group.
- HTTP and HTTPS listeners cannot share the same port yet.

### CLI

```bash
# Serve
rustproxy serve config.yaml

# Users
rustproxy user add admin
rustproxy user list
rustproxy user passwd admin

# Global config
rustproxy config get
rustproxy config get proxy_listen
rustproxy config set proxy_listen 0.0.0.0:8080

# Upstreams
rustproxy config upstream list
rustproxy config upstream add api http://127.0.0.1:8081 --weight 100
rustproxy config upstream add-target api http://127.0.0.1:8082 --weight 50
rustproxy config upstream delete api

# Rules
rustproxy config rule list
rustproxy config rule add rule-api --name "API" --upstream api --priority 100 --listen 0.0.0.0:80 --host-type exact --host api.example.com --location-type prefix --location /api --condition-type header --key x-tenant --operator exact --value a
rustproxy config rule delete rule-api

# Import / export
rustproxy config export
rustproxy config import config.yaml --replace
rustproxy config edit
```

### Development

```bash
# Frontend dev server
cd ui
npm install
npm run dev

# Frontend production build
npm run build

# Backend checks and tests
cd ..
cargo check
cargo test
```

### Metrics

RustProxy exposes Prometheus metrics at:

```text
http://0.0.0.0:3000/metrics
```

Useful metric groups include:

- proxy request counters
- request latency histograms
- active connection gauges
- config reload counters
- process memory, CPU time, and file descriptor gauges
