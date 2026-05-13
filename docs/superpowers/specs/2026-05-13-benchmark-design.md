# Benchmark Design: rustproxy vs nginx vs haproxy

## Goal

Design a reproducible benchmark suite that runs on a **local Linux machine**, comparing rustproxy against nginx and haproxy across three core scenarios. Results are collected into a **markdown report**.

## Environment

- **Platform**: Local Linux (bare metal or VM, no Docker)
- **Tooling**: `wrk2` (primary, constant-rate latency) + `wrk` (max throughput)
- **Backend**: nginx serving a 256-byte static file on ports 8001-8004 (50k+ RPS capability, eliminates backend bottleneck)
- **rustproxy build**: `cargo build --release`

## Test Scenarios

### Scenario 1: Passthrough

Measure raw proxy forwarding with zero routing logic.

- All proxies configured with a single rule forwarding to one backend
- No header matching, no path matching, no load balancing

### Scenario 2: Routing (10 rules)

Measure condition matching overhead.

10 rules with equivalent logic across all three proxies:

| # | Condition | Target Backend |
|---|-----------|---------------|
| 1 | Host exact: `api.example.com` | 8001 |
| 2 | Host exact: `web.example.com` | 8002 |
| 3 | Path prefix: `/v1/` | 8003 |
| 4 | Path prefix: `/v2/` | 8004 |
| 5 | Header exists: `X-Api-Key` | 8001 |
| 6 | Header exact: `X-Version=2` | 8002 |
| 7 | Cookie exists: `session` | 8003 |
| 8 | Cookie exact: `env=prod` | 8004 |
| 9 | Host regex: `.*\.cdn\.example\.com` | 8001 |
| 10 | Path prefix: `/health` | 8002 |

- nginx: 10 `location` blocks with equivalent conditions
- haproxy: 10 `acl` + `use_backend` rules
- rustproxy: 10 rules with matching `conditions`

### Scenario 3: Load Balancing (4 backends, weighted)

Measure weighted round-robin scheduling overhead.

- 4 backend instances on ports 8001-8004
- Weights: 4:3:2:1
- Single rule, no condition matching

## Benchmark Parameters

For each scenario × each proxy:

| Mode | wrk2 command | Purpose |
|------|-------------|---------|
| **Max throughput** | `wrk -c 200 -t $(nproc) -d 30s` | Push as hard as possible |
| **Constant 10k RPS** | `wrk2 -c 200 -t $(nproc) -d 30s -R 10000` | Latency at fixed 10k req/s |
| **Constant 20k RPS** | `wrk2 -c 200 -t $(nproc) -d 30s -R 20000` | Latency at fixed 20k req/s |

- 5-second warmup before each run (not counted)
- Proxies run serially (never concurrently) to avoid resource contention
- 2-second pause between proxy runs for system recovery

## Project Structure

```
bench/
├── run.sh                         # Main entry: setup → run all scenarios → generate report
├── setup.sh                       # Install deps: wrk/wrk2, nginx, haproxy
├── configs/
│   ├── rustproxy/
│   │   ├── passthrough.yaml
│   │   ├── routing.yaml
│   │   └── loadbalance.yaml
│   ├── nginx/
│   │   ├── passthrough.conf
│   │   ├── routing.conf
│   │   └── loadbalance.conf
│   └── haproxy/
│       ├── passthrough.cfg
│       ├── routing.cfg
│       └── loadbalance.cfg
├── tools/
│   └── backend.conf               # nginx static file server config (ports 8001-8004)
│   └── response.txt               # 256-byte static response body
└── report.sh                      # Parse wrk2 output → markdown tables
```

## Runtime Flow

```
1. setup.sh            → check/install deps, build rustproxy --release
2. Start backends      → nginx -c backend.conf (serves static 256B file on 8001-8004)
3. For scenario in passthrough routing loadbalance:
    For proxy in nginx haproxy rustproxy:
      a. Write/load equivalent config
      b. Start proxy
         - nginx:      port 9001
         - haproxy:    port 9002
         - rustproxy:  port 9003
      c. Sleep 1s (wait for ready)
      d. wrk warmup 5s
      e. wrk/wrk2 formal run → collect output
      f. Stop proxy
    Done
  Done
4. report.sh           → parse outputs → generate markdown report
5. Stop backends
```

## Fairness Controls

- **Serial execution**: one proxy at a time, no resource competition
- **Same backend**: all proxies forward to identical backend servers
- **Loopback networking**: wrk2 client ↔ proxy on localhost, zero network variance
- **Release build**: `cargo build --release` for rustproxy
- **Pause between runs**: 2s gap for system recovery
- **Equivalent configs**: all three proxy configs produce identical routing behavior per scenario

## Collected Metrics

Per run (wrk2 output):

- Requests/sec (throughput)
- Latency: avg, stddev, p50, p75, p90, p99, p99.9
- Transfer/sec (MB/s)
- Socket errors (if any)

## Report Format

Markdown with one table per scenario × mode:

```markdown
## Passthrough - Max Throughput

| Metric         | nginx    | haproxy  | rustproxy |
|----------------|----------|----------|-----------|
| Requests/sec   | ...      | ...      | ...       |
| Avg latency    | ...      | ...      | ...       |
| p50            | ...      | ...      | ...       |
| p90            | ...      | ...      | ...       |
| p99            | ...      | ...      | ...       |
| p99.9          | ...      | ...      | ...       |
| Transfer/sec   | ...      | ...      | ...       |

## Passthrough - Latency @ 10k RPS
...
## Passthrough - Latency @ 20k RPS
...
## Routing - Max Throughput
...
(and so on for all 9 combinations)
```

## Dependencies

- `wrk` / `wrk2` (build from source if not in package manager)
- `nginx` (apt/yum install) — dual role: backend static server + benchmark target
- `haproxy` (apt/yum install)
- `cargo` + Rust toolchain (for rustproxy)
