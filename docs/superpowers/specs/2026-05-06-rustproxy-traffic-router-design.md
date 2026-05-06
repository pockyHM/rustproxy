# RustProxy Traffic Router — Design Specification

## Overview

A high-performance Rust-based traffic routing middleware (reverse proxy) with a web UI for configuration management. Inspired by HAProxy, designed for fine-grained request routing based on HTTP headers, cookies, and JWT claims.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      User Browser                            │
│                   (React + Vite Web UI)                      │
└─────────────────┬───────────────────────────────────────────┘
                  │ HTTPS (REST API)
                  ▼
┌─────────────────────────────────────────────────────────────┐
│                     Rust API Server                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │  axum REST   │  │  Rule Engine │  │  Prometheus API   │  │
│  │    API       │  │  (Matcher)   │  │   /metrics       │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────────────┘  │
│         │                 │                                  │
│         ▼                 ▼                                  │
│  ┌──────────────────────────────────┐                       │
│  │       YAML Configuration File     │                       │
│  │        (config.yaml)              │                       │
│  └──────────────────────────────────┘                       │
└─────────────────┬───────────────────────────────────────────┘
                  │ Request Forwarding
                  ▼
┌─────────────────────────────────────────────────────────────┐
│                  Routing Proxy Layer                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │  Header Match│  │ Cookie Match │  │ JWT Parse/Match  │  │
│  └──────────────┘  └──────────────┘  └──────────────────┘  │
│                                                              │
│  ┌──────────────┐  ┌──────────────┐                          │
│  │ Load Balancer│  │   Fallback   │                          │
│  │(Weighted RR) │  │  Upstream    │                          │
│  └──────────────┘  └──────────────┘                          │
└─────────────────────────────────────────────────────────────┘
                  │
                  ▼
         ┌────────────────┐
         │ Target Upstream │  (Address or K8s Pod — Phase 2)
         └────────────────┘
```

## Components

### 1. Configuration Management

**File**: `config.yaml`

**Structure**:
```yaml
version: "1"
listen: "0.0.0.0:8080"

rules:
  - name: "header-rule-1"
    priority: 100
    conditions:
      - type: header
        key: "X-Tenant-ID"
        operator: regex
        value: "^tenant-[a-z]+$"
    upstream: "backend-1"
    weight: 10

  - name: "cookie-rule-1"
    priority: 90
    conditions:
      - type: cookie
        key: "session_id"
        operator: exact
        value: "abc123"
    upstream: "backend-2"
    weight: 5

  - name: "jwt-rule-1"
    priority: 80
    conditions:
      - type: jwt
        claim_path: "department"
        operator: regex
        value: "^engineering.*$"
    upstream: "backend-3"
    weight: 5

upstreams:
  backend-1:
    targets:
      - url: "http://192.168.1.10:8080"
        weight: 10
      - url: "http://192.168.1.11:8080"
        weight: 5

  backend-2:
    targets:
      - url: "http://192.168.1.20:8080"
        weight: 10

  backend-3:
    targets:
      - url: "http://192.168.1.30:8080"
        weight: 10

fallback:
  url: "http://192.168.1.100:8080"
```

**Hot Reload**: On config file change, reload automatically (via file watcher).

### 2. Rule Engine

**Matching Logic**:
- All conditions within a rule must match (AND logic)
- Rules are evaluated by priority (higher first)
- First matching rule determines target upstream
- If no rule matches, use `fallback`

**Supported Condition Types**:

| Type | Description |
|------|-------------|
| `header` | Match against HTTP request headers |
| `cookie` | Match against HTTP cookies |
| `jwt` | Parse JWT, match against specified claim path |

**Operators**:

| Operator | Applicable To | Description |
|----------|---------------|-------------|
| `exact` | header, cookie, jwt | Exact string match |
| `regex` | header, cookie, jwt | Regular expression match |
| `exists` | header, cookie | Check if key exists (any value) |
| `contains` | header, cookie, jwt | Substring match |

**JWT Handling**:
- Parse JWT without validation (trust terminating proxy's validation)
- Support nested claim paths: `user.metadata.tenant_id`
- Extract claims from standard locations (payload, not signature)

### 3. Load Balancer

- **Algorithm**: Weighted Round-Robin
- **Connection Pool**: Reuse connections to upstream targets
- **Health**: Targets are always considered healthy (no active health checks in Phase 1)

### 4. API Server (axum)

**Endpoints**:

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/config` | Get current configuration |
| PUT | `/api/config` | Update configuration |
| GET | `/api/rules` | List all rules |
| POST | `/api/rules` | Create new rule |
| PUT | `/api/rules/:id` | Update rule |
| DELETE | `/api/rules/:id` | Delete rule |
| GET | `/api/upstreams` | List upstreams |
| POST | `/api/upstreams` | Create upstream |
| PUT | `/api/upstreams/:id` | Update upstream |
| DELETE | `/api/upstreams/:id` | Delete upstream |
| GET | `/api/health` | Health check |
| GET | `/metrics` | Prometheus metrics |

**Response Format**: JSON
```json
{
  "success": true,
  "data": { ... }
}
```

### 5. Observability

**Prometheus Metrics**:
- `proxy_requests_total{rule="...", upstream="...", status="..."}`
- `proxy_request_duration_seconds{rule="...", upstream="..."}`
- `proxy_active_connections`
- `proxy_config_reloads_total`

**Structured Logging** (tracing + OpenTelemetry):
- Request ID (UUID) for distributed tracing
- Log fields: rule matched, upstream selected, request duration
- Export to stdout / OTLP endpoint (configurable)

### 6. Web UI (React + Vite)

**Features**:
- Dashboard: overview of rules, upstreams, traffic stats
- Rule Editor: visual rule builder with condition组合
- Upstream Editor: add/edit backend targets
- YAML Editor: raw config editing mode
- Real-time validation
- Import/Export configuration

## Phase 1 Scope

- [x] Header condition matching (exact, regex, exists, contains)
- [x] Cookie condition matching (exact, regex, exists, contains)
- [x] JWT condition matching with claim path (exact, regex, contains)
- [x] Weighted Round-Robin load balancing
- [x] Fallback upstream
- [x] YAML configuration file with hot reload
- [x] REST API for configuration
- [x] Web UI for configuration
- [x] Prometheus metrics
- [x] OpenTelemetry structured logging

## Phase 2 Scope (Future)

- K8s Pod routing based on label selectors
- Dynamic service discovery
- Active health checks
- Rate limiting
- mTLS upstream connections

## Technology Stack

| Layer | Technology |
|-------|------------|
| Proxy Core | `hyper` + `tokio` |
| HTTP Server | `axum` |
| Config File | `serde_yaml` |
| JWT Parsing | `jsonwebtoken` |
| Regex | `regex` |
| Metrics | `prometheus` + `metrics` |
| Logging | `tracing` + `tracing-opentelemetry` |
| Web UI | React + Vite + TypeScript |
| Config Watcher | `notify` |

## File Structure

```
rustproxy/
├── Cargo.toml
├── config.yaml
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── config/
│   │   ├── mod.rs
│   │   ├── yaml.rs
│   │   └── watcher.rs
│   ├── api/
│   │   ├── mod.rs
│   │   ├── handlers.rs
│   │   └── routes.rs
│   ├── proxy/
│   │   ├── mod.rs
│   │   ├── matcher.rs
│   │   ├── conditions/
│   │   │   ├── mod.rs
│   │   │   ├── header.rs
│   │   │   ├── cookie.rs
│   │   │   └── jwt.rs
│   │   ├── balancer.rs
│   │   └── upstream.rs
│   ├── observability/
│   │   ├── mod.rs
│   │   ├── metrics.rs
│   │   └── tracing.rs
│   └── models/
│       ├── mod.rs
│       ├── rule.rs
│       └── upstream.rs
├── ui/                     # React frontend
│   ├── package.json
│   ├── vite.config.ts
│   └── src/
│       ├── App.tsx
│       ├── components/
│       └── pages/
└── docs/
    └── specs/
        └── 2026-05-06-rustproxy-traffic-router-design.md
```
