# Benchmark Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a reproducible benchmark suite comparing rustproxy vs nginx vs haproxy across passthrough, routing, and load balancing scenarios.

**Architecture:** Shell-script driven. `setup.sh` installs deps, `run.sh` orchestrates serial execution of all scenarios, `report.sh` parses wrk2 output into a markdown comparison report. All configs are pre-generated static files.

**Tech Stack:** Bash, wrk2, wrk, nginx, haproxy, rustproxy (--release)

---

## File Map

| File | Responsibility |
|------|---------------|
| `bench/setup.sh` | Install wrk/wrk2, nginx, haproxy; build rustproxy |
| `bench/tools/backend.conf` | nginx config serving static file on ports 8001-8004 |
| `bench/tools/response.txt` | Exactly 256-byte static response body |
| `bench/configs/rustproxy/passthrough.yaml` | Single-rule fallback to 8001 |
| `bench/configs/rustproxy/routing.yaml` | 10 conditional rules |
| `bench/configs/rustproxy/loadbalance.yaml` | Weighted 4:3:2:1 across 8001-8004 |
| `bench/configs/nginx/passthrough.conf` | Single upstream proxy_pass |
| `bench/configs/nginx/routing.conf` | map/if rules equivalent to 10-condition spec |
| `bench/configs/nginx/loadbalance.conf` | upstream with 4 weighted servers |
| `bench/configs/haproxy/passthrough.cfg` | Single backend server |
| `bench/configs/haproxy/routing.cfg` | 10 ACL + use_backend rules |
| `bench/configs/haproxy/loadbalance.cfg` | backend with 4 weighted servers |
| `bench/report.sh` | Parse raw wrk2 outputs → markdown tables |
| `bench/run.sh` | Main entry: setup → run → report |

---

### Task 1: Backend Infrastructure

**Files:**
- Create: `bench/tools/response.txt`
- Create: `bench/tools/backend.conf`

- [ ] **Step 1: Create 256-byte response body**

```bash
mkdir -p bench/tools
python3 -c "print('X' * 255)" > bench/tools/response.txt
# verify exactly 256 bytes (255 X's + newline)
wc -c bench/tools/response.txt
```

Expected: `256 bench/tools/response.txt`

- [ ] **Step 2: Create nginx backend config serving on 4 ports**

Create `bench/tools/backend.conf`:

```nginx
worker_processes auto;
pid /tmp/bench_backend.pid;
error_log /tmp/bench_backend_error.log;

events {
    worker_connections 1024;
}

http {
    access_log off;

    server {
        listen 8001;
        listen 8002;
        listen 8003;
        listen 8004;
        server_name _;

        root /tmp/bench_www;
        index response.txt;

        location / {
            try_files $uri /response.txt =404;
        }
    }
}
```

- [ ] **Step 3: Verify backend works**

```bash
# Copy response file to serve dir
mkdir -p /tmp/bench_www
cp bench/tools/response.txt /tmp/bench_www/response.txt

# Start backend
nginx -c $(pwd)/bench/tools/backend.conf
sleep 1

# Test
curl -s http://127.0.0.1:8001/response.txt | wc -c

# Stop
nginx -c $(pwd)/bench/tools/backend.conf -s stop
```

Expected: `256`

- [ ] **Step 4: Commit**

```bash
git add bench/tools/response.txt bench/tools/backend.conf
git commit -m "bench: add backend nginx static server for benchmarks"
```

---

### Task 2: Setup Script

**Files:**
- Create: `bench/setup.sh`

- [ ] **Step 1: Write setup.sh**

Create `bench/setup.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

echo "=== Benchmark Setup ==="

# Install system deps
if command -v apt-get &>/dev/null; then
    sudo apt-get update -qq
    sudo apt-get install -y -qq nginx haproxy build-essential libssl-dev git > /dev/null
elif command -v yum &>/dev/null; then
    sudo yum install -y nginx haproxy gcc make openssl-devel git
else
    echo "Unsupported package manager. Install nginx, haproxy, gcc, make, openssl-dev manually."
    exit 1
fi

# Install wrk
if ! command -v wrk &>/dev/null; then
    echo "Building wrk..."
    cd /tmp && git clone https://github.com/wg/wrk.git 2>/dev/null || true
    cd /tmp/wrk && make -j$(nproc) && sudo cp wrk /usr/local/bin/
    echo "wrk installed."
fi

# Install wrk2
if ! command -v wrk2 &>/dev/null; then
    echo "Building wrk2..."
    cd /tmp && git clone https://github.com/giltene/wrk2.git 2>/dev/null || true
    cd /tmp/wrk2 && make -j$(nproc) && sudo cp wrk /usr/local/bin/wrk2
    echo "wrk2 installed."
fi

# Build rustproxy release
echo "Building rustproxy --release..."
cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
cargo build --release

echo "=== Setup Complete ==="
```

- [ ] **Step 2: Make executable and verify**

```bash
chmod +x bench/setup.sh
bash -n bench/setup.sh  # syntax check
```

Expected: no output (clean syntax)

- [ ] **Step 3: Commit**

```bash
git add bench/setup.sh
git commit -m "bench: add setup script for dependencies and build"
```

---

### Task 3: Passthrough Configs

**Files:**
- Create: `bench/configs/rustproxy/passthrough.yaml`
- Create: `bench/configs/nginx/passthrough.conf`
- Create: `bench/configs/haproxy/passthrough.cfg`

- [ ] **Step 1: Create rustproxy passthrough config**

Create `bench/configs/rustproxy/passthrough.yaml`:

```yaml
version: "1.0"
listen: "127.0.0.1:3000"
proxy_listen: "0.0.0.0:9003"
skip_ssl: true
rules: []
upstreams: {}
fallback:
  url: "http://127.0.0.1:8001"
```

- [ ] **Step 2: Create nginx passthrough config**

Create `bench/configs/nginx/passthrough.conf`:

```nginx
worker_processes auto;
pid /tmp/bench_nginx_proxy.pid;
error_log /tmp/bench_nginx_error.log;

events {
    worker_connections 1024;
}

http {
    access_log off;

    upstream backend {
        server 127.0.0.1:8001;
    }

    server {
        listen 9001;
        location / {
            proxy_pass http://backend;
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_http_version 1.1;
            proxy_set_header Connection "";
        }
    }
}
```

- [ ] **Step 3: Create haproxy passthrough config**

Create `bench/configs/haproxy/passthrough.cfg`:

```
global
    log /dev/null local0

defaults
    mode http
    timeout client 30s
    timeout server 30s
    timeout connect 5s
    log global
    option dontlognull
    option dontlog-normal

frontend proxy
    bind 127.0.0.1:9002
    default_backend backend

backend backend
    server s1 127.0.0.1:8001
```

- [ ] **Step 4: Verify all three configs are syntactically valid**

```bash
# nginx syntax check
nginx -t -c $(pwd)/bench/configs/nginx/passthrough.conf 2>&1 || true

# haproxy syntax check
haproxy -c -f bench/configs/haproxy/passthrough.cfg 2>&1 || true
```

Expected: Both report "syntax is ok" or "configuration file ... syntax ok"

- [ ] **Step 5: Commit**

```bash
git add bench/configs/rustproxy/passthrough.yaml bench/configs/nginx/passthrough.conf bench/configs/haproxy/passthrough.cfg
git commit -m "bench: add passthrough configs for all proxies"
```

---

### Task 4: Routing Configs (10 rules)

**Files:**
- Create: `bench/configs/rustproxy/routing.yaml`
- Create: `bench/configs/nginx/routing.conf`
- Create: `bench/configs/haproxy/routing.cfg`

- [ ] **Step 1: Create rustproxy routing config**

Create `bench/configs/rustproxy/routing.yaml`:

```yaml
version: "1.0"
listen: "127.0.0.1:3000"
proxy_listen: "0.0.0.0:9003"
skip_ssl: true
rules:
  - id: "r1"
    name: "Host api"
    priority: 100
    conditions:
      - type: host
        operator: exact
        value: "api.example.com"
    upstream: "b8001"
    weight: 100
  - id: "r2"
    name: "Host web"
    priority: 100
    conditions:
      - type: host
        operator: exact
        value: "web.example.com"
    upstream: "b8002"
    weight: 100
  - id: "r3"
    name: "Path /v1/"
    priority: 90
    conditions:
      - type: path
        operator: prefix
        value: "/v1/"
    upstream: "b8003"
    weight: 100
  - id: "r4"
    name: "Path /v2/"
    priority: 90
    conditions:
      - type: path
        operator: prefix
        value: "/v2/"
    upstream: "b8004"
    weight: 100
  - id: "r5"
    name: "Header X-Api-Key exists"
    priority: 80
    conditions:
      - type: header
        key: "X-Api-Key"
        operator: exists
    upstream: "b8001"
    weight: 100
  - id: "r6"
    name: "Header X-Version=2"
    priority: 80
    conditions:
      - type: header
        key: "X-Version"
        operator: exact
        value: "2"
    upstream: "b8002"
    weight: 100
  - id: "r7"
    name: "Cookie session exists"
    priority: 70
    conditions:
      - type: cookie
        key: "session"
        operator: exists
    upstream: "b8003"
    weight: 100
  - id: "r8"
    name: "Cookie env=prod"
    priority: 70
    conditions:
      - type: cookie
        key: "env"
        operator: exact
        value: "prod"
    upstream: "b8004"
    weight: 100
  - id: "r9"
    name: "Host CDN regex"
    priority: 60
    conditions:
      - type: host
        operator: regex
        value: ".*\\.cdn\\.example\\.com"
    upstream: "b8001"
    weight: 100
  - id: "r10"
    name: "Path /health"
    priority: 50
    conditions:
      - type: path
        operator: prefix
        value: "/health"
    upstream: "b8002"
    weight: 100
upstreams:
  b8001:
    name: "b8001"
    targets:
      - url: "http://127.0.0.1:8001"
        weight: 100
  b8002:
    name: "b8002"
    targets:
      - url: "http://127.0.0.1:8002"
        weight: 100
  b8003:
    name: "b8003"
    targets:
      - url: "http://127.0.0.1:8003"
        weight: 100
  b8004:
    name: "b8004"
    targets:
      - url: "http://127.0.0.1:8004"
        weight: 100
fallback:
  url: "http://127.0.0.1:8001"
```

- [ ] **Step 2: Create nginx routing config**

nginx uses `map` blocks for header/cookie matching and `location` for path matching.

Create `bench/configs/nginx/routing.conf`:

```nginx
worker_processes auto;
pid /tmp/bench_nginx_proxy.pid;
error_log /tmp/bench_nginx_error.log;

events {
    worker_connections 1024;
}

http {
    access_log off;

    upstream b8001 { server 127.0.0.1:8001; }
    upstream b8002 { server 127.0.0.1:8002; }
    upstream b8003 { server 127.0.0.1:8003; }
    upstream b8004 { server 127.0.0.1:8004; }

    map $host $host_backend {
        api.example.com         b8001;
        web.example.com          b8002;
        default                 "";
    }

    map $http_x_api_key $header_api_backend {
        ""       "";
        default  b8001;
    }

    map $http_x_version $header_version_backend {
        "2"      b8002;
        default  "";
    }

    map $cookie_session $cookie_session_backend {
        ""       "";
        default  b8003;
    }

    map $cookie_env $cookie_env_backend {
        "prod"   b8004;
        default  "";
    }

    map $host $cdn_backend {
        ~.*\.cdn\.example\.com  b8001;
        default                "";
    }

    server {
        listen 9001;

        # R10: /health
        location /health {
            proxy_pass http://b8002;
            proxy_set_header Host $host;
            proxy_http_version 1.1;
            proxy_set_header Connection "";
        }

        # R3: /v1/
        location /v1/ {
            proxy_pass http://b8003;
            proxy_set_header Host $host;
            proxy_http_version 1.1;
            proxy_set_header Connection "";
        }

        # R4: /v2/
        location /v2/ {
            proxy_pass http://b8004;
            proxy_set_header Host $host;
            proxy_http_version 1.1;
            proxy_set_header Connection "";
        }

        # R1,R2,R5,R6,R7,R8,R9: host/header/cookie matching
        location / {
            set $target "";

            # R1,R2: Host exact
            if ($host_backend != "") {
                set $target $host_backend;
            }

            # R9: Host regex CDN
            if ($cdn_backend != "") {
                set $target $cdn_backend;
            }

            # R5: X-Api-Key exists
            if ($header_api_backend != "") {
                set $target $header_api_backend;
            }

            # R6: X-Version=2
            if ($header_version_backend != "") {
                set $target $header_version_backend;
            }

            # R7: Cookie session exists
            if ($cookie_session_backend != "") {
                set $target $cookie_session_backend;
            }

            # R8: Cookie env=prod
            if ($cookie_env_backend != "") {
                set $target $cookie_env_backend;
            }

            # default
            if ($target = "") {
                set $target b8001;
            }

            proxy_pass http://$target;
            proxy_set_header Host $host;
            proxy_http_version 1.1;
            proxy_set_header Connection "";
        }
    }
}
```

- [ ] **Step 3: Create haproxy routing config**

Create `bench/configs/haproxy/routing.cfg`:

```
global
    log /dev/null local0

defaults
    mode http
    timeout client 30s
    timeout server 30s
    timeout connect 5s
    log global
    option dontlognull
    option dontlog-normal

frontend proxy
    bind 127.0.0.1:9002

    # R1: Host exact api.example.com
    acl host_api hdr(host) -i api.example.com
    # R2: Host exact web.example.com
    acl host_web hdr(host) -i web.example.com
    # R3: Path prefix /v1/
    acl path_v1 path_beg /v1/
    # R4: Path prefix /v2/
    acl path_v2 path_beg /v2/
    # R5: Header X-Api-Key exists
    acl hdr_apikey hdr(X-Api-Key) -m found
    # R6: Header X-Version exact 2
    acl hdr_version hdr(X-Version) 2
    # R7: Cookie session exists
    acl cookie_session cook(session) -m found
    # R8: Cookie env exact prod
    acl cookie_env cook(env) prod
    # R9: Host regex CDN
    acl host_cdn hdr(host) -m reg .*\.cdn\.example\.com
    # R10: Path prefix /health
    acl path_health path_beg /health

    use_backend b8001 if host_api
    use_backend b8002 if host_web
    use_backend b8003 if path_v1
    use_backend b8004 if path_v2
    use_backend b8001 if hdr_apikey
    use_backend b8002 if hdr_version
    use_backend b8003 if cookie_session
    use_backend b8004 if cookie_env
    use_backend b8001 if host_cdn
    use_backend b8002 if path_health
    default_backend b8001

backend b8001
    server s1 127.0.0.1:8001

backend b8002
    server s1 127.0.0.1:8002

backend b8003
    server s1 127.0.0.1:8003

backend b8004
    server s1 127.0.0.1:8004
```

- [ ] **Step 4: Verify configs syntactically**

```bash
nginx -t -c $(pwd)/bench/configs/nginx/routing.conf 2>&1 || true
haproxy -c -f bench/configs/haproxy/routing.cfg 2>&1 || true
```

Expected: Both report syntax ok

- [ ] **Step 5: Commit**

```bash
git add bench/configs/rustproxy/routing.yaml bench/configs/nginx/routing.conf bench/configs/haproxy/routing.cfg
git commit -m "bench: add routing configs (10 rules) for all proxies"
```

---

### Task 5: Load Balancing Configs

**Files:**
- Create: `bench/configs/rustproxy/loadbalance.yaml`
- Create: `bench/configs/nginx/loadbalance.conf`
- Create: `bench/configs/haproxy/loadbalance.cfg`

- [ ] **Step 1: Create rustproxy loadbalance config**

Create `bench/configs/rustproxy/loadbalance.yaml`:

```yaml
version: "1.0"
listen: "127.0.0.1:3000"
proxy_listen: "0.0.0.0:9003"
skip_ssl: true
rules:
  - id: "lb"
    name: "Load balance"
    priority: 1
    conditions: []
    upstream: "weighted"
    weight: 100
upstreams:
  weighted:
    name: "weighted"
    targets:
      - url: "http://127.0.0.1:8001"
        weight: 4
      - url: "http://127.0.0.1:8002"
        weight: 3
      - url: "http://127.0.0.1:8003"
        weight: 2
      - url: "http://127.0.0.1:8004"
        weight: 1
fallback:
  url: "http://127.0.0.1:8001"
```

- [ ] **Step 2: Create nginx loadbalance config**

Create `bench/configs/nginx/loadbalance.conf`:

```nginx
worker_processes auto;
pid /tmp/bench_nginx_proxy.pid;
error_log /tmp/bench_nginx_error.log;

events {
    worker_connections 1024;
}

http {
    access_log off;

    upstream weighted {
        server 127.0.0.1:8001 weight=4;
        server 127.0.0.1:8002 weight=3;
        server 127.0.0.1:8003 weight=2;
        server 127.0.0.1:8004 weight=1;
    }

    server {
        listen 9001;
        location / {
            proxy_pass http://weighted;
            proxy_set_header Host $host;
            proxy_http_version 1.1;
            proxy_set_header Connection "";
        }
    }
}
```

- [ ] **Step 3: Create haproxy loadbalance config**

Create `bench/configs/haproxy/loadbalance.cfg`:

```
global
    log /dev/null local0

defaults
    mode http
    timeout client 30s
    timeout server 30s
    timeout connect 5s
    log global
    option dontlognull
    option dontlog-normal

frontend proxy
    bind 127.0.0.1:9002
    default_backend weighted

backend weighted
    balance roundrobin
    server s1 127.0.0.1:8001 weight 4
    server s2 127.0.0.1:8002 weight 3
    server s3 127.0.0.1:8003 weight 2
    server s4 127.0.0.1:8004 weight 1
```

- [ ] **Step 4: Verify configs syntactically**

```bash
nginx -t -c $(pwd)/bench/configs/nginx/loadbalance.conf 2>&1 || true
haproxy -c -f bench/configs/haproxy/loadbalance.cfg 2>&1 || true
```

Expected: Both report syntax ok

- [ ] **Step 5: Commit**

```bash
git add bench/configs/rustproxy/loadbalance.yaml bench/configs/nginx/loadbalance.conf bench/configs/haproxy/loadbalance.cfg
git commit -m "bench: add load balancing configs (4:3:2:1) for all proxies"
```

---

### Task 6: Report Generator

**Files:**
- Create: `bench/report.sh`

- [ ] **Step 1: Write report.sh**

Create `bench/report.sh`:

```bash
#!/usr/bin/env bash
# Parse wrk/wrk2 raw output files and produce a markdown comparison report.
# Usage: report.sh <output_dir> <report_file>
#
# Expected file layout in output_dir:
#   <scenario>/<proxy>/<mode>.txt   (e.g. passthrough/nginx/throughput.txt)

set -euo pipefail

OUTPUT_DIR="${1:?Usage: report.sh <output_dir> <report_file>}"
REPORT="${2:?Usage: report.sh <output_dir> <report_file>}"

PROXIES="nginx haproxy rustproxy"
SCENARIOS="passthrough routing loadbalance"
MODES="throughput latency_10k latency_20k"
MODE_LABELS="throughput:Max\ Throughput latency_10k:Latency\ @\ 10k\ RPS latency_20k:Latency\ @\ 20k\ RPS"

extract_metric() {
    local file="$1" metric="$2"
    if [[ ! -f "$file" ]]; then
        echo "N/A"
        return
    fi
    case "$metric" in
        rps)
            grep -oP 'Requests/sec:\s+\K[\d.]+' "$file" || echo "N/A"
            ;;
        avg)
            grep -oP 'Latency\s+\K[\d.]+us|[\d.]+ms|[\d.]+s' "$file" | head -1 || echo "N/A"
            ;;
        p50)
            grep -oP '50\.000%\s+\K[\d.]+us|[\d.]+ms|[\d.]+s' "$file" || echo "N/A"
            ;;
        p90)
            grep -oP '90\.000%\s+\K[\d.]+us|[\d.]+ms|[\d.]+s' "$file" || echo "N/A"
            ;;
        p99)
            grep -oP '99\.000%\s+\K[\d.]+us|[\d.]+ms|[\d.]+s' "$file" || echo "N/A"
            ;;
        p999)
            grep -oP '99\.900%\s+\K[\d.]+us|[\d.]+ms|[\d.]+s' "$file" || echo "N/A"
            ;;
        transfer)
            grep -oP 'Transfer/sec:\s+\K[\d.]+[A-Z]+B' "$file" || echo "N/A"
            ;;
    esac
}

{
    echo "# Benchmark Report: rustproxy vs nginx vs haproxy"
    echo ""
    echo "Generated: $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    echo ""

    for scenario in $SCENARIOS; do
        for mode_label in $MODE_LABELS; do
            mode="${mode_label%%:*}"
            label="${mode_label##*:}"
            echo "## ${scenario^} - ${label}"
            echo ""
            echo "| Metric | nginx | haproxy | rustproxy |"
            echo "|--------|-------|---------|-----------|"

            for metric in rps avg p50 p90 p99 p999 transfer; do
                row="| ${metric^^} |"
                for proxy in $PROXIES; do
                    file="${OUTPUT_DIR}/${scenario}/${proxy}/${mode}.txt"
                    val=$(extract_metric "$file" "$metric")
                    row="${row} ${val} |"
                done
                echo "$row"
            done
            echo ""
        done
    done
} > "$REPORT"

echo "Report written to: ${REPORT}"
```

- [ ] **Step 2: Make executable and syntax check**

```bash
chmod +x bench/report.sh
bash -n bench/report.sh
```

Expected: no output (clean syntax)

- [ ] **Step 3: Commit**

```bash
git add bench/report.sh
git commit -m "bench: add report generator for wrk2 output parsing"
```

---

### Task 7: Main Runner

**Files:**
- Create: `bench/run.sh`

- [ ] **Step 1: Write run.sh**

Create `bench/run.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUTPUT_DIR="${PROJECT_ROOT}/bench/results"
THREADS=$(nproc)
CONNECTIONS=200
DURATION=30
WARMUP=5

PROXIES="nginx haproxy rustproxy"
SCENARIOS="passthrough routing loadbalance"

PROXY_PORTS="nginx:9001 haproxy:9002 rustproxy:9003"

die() { echo "ERROR: $*" >&2; exit 1; }

# --- helpers ---

start_backend() {
    mkdir -p /tmp/bench_www
    cp "${SCRIPT_DIR}/tools/response.txt" /tmp/bench_www/response.txt
    nginx -c "${SCRIPT_DIR}/tools/backend.conf" 2>/dev/null || die "Failed to start backend nginx"
    sleep 1
    echo "Backend started (ports 8001-8004)"
}

stop_backend() {
    nginx -c "${SCRIPT_DIR}/tools/backend.conf" -s stop 2>/dev/null || true
    echo "Backend stopped"
}

start_proxy() {
    local proxy="$1" scenario="$2"
    local config
    case "$proxy" in
        nginx)
            config="${SCRIPT_DIR}/configs/nginx/${scenario}.conf"
            nginx -c "$config" 2>/dev/null || die "Failed to start nginx proxy"
            ;;
        haproxy)
            config="${SCRIPT_DIR}/configs/haproxy/${scenario}.cfg"
            haproxy -f "$config" -D -p /tmp/bench_haproxy.pid 2>/dev/null || die "Failed to start haproxy proxy"
            ;;
        rustproxy)
            config="${SCRIPT_DIR}/configs/rustproxy/${scenario}.yaml"
            "${PROJECT_ROOT}/target/release/rustproxy" serve --config "$config" &>/tmp/bench_rustproxy.log &
            echo $! > /tmp/bench_rustproxy.pid
            ;;
    esac
    sleep 1
}

stop_proxy() {
    local proxy="$1" scenario="$2"
    case "$proxy" in
        nginx)
            local config="${SCRIPT_DIR}/configs/nginx/${scenario}.conf"
            nginx -c "$config" -s stop 2>/dev/null || true
            ;;
        haproxy)
            [[ -f /tmp/bench_haproxy.pid ]] && kill "$(cat /tmp/bench_haproxy.pid)" 2>/dev/null || true
            ;;
        rustproxy)
            [[ -f /tmp/bench_rustproxy.pid ]] && kill "$(cat /tmp/bench_rustproxy.pid)" 2>/dev/null || true
            ;;
    esac
    sleep 2
}

get_port() {
    for entry in $PROXY_PORTS; do
        if [[ "${entry%%:*}" == "$1" ]]; then
            echo "${entry##*:}"
            return
        fi
    done
    die "Unknown proxy: $1"
}

run_bench() {
    local proxy="$1" scenario="$2" mode="$3" output_file="$4"
    local port
    port=$(get_port "$proxy")
    local url="http://127.0.0.1:${port}/response.txt"

    echo "  Running ${mode} for ${proxy} on port ${port}..."

    # Warmup
    wrk -c "${CONNECTIONS}" -t "${THREADS}" -d "${WARMUP}s" "${url}" &>/dev/null || true
    sleep 1

    case "$mode" in
        throughput)
            wrk -c "${CONNECTIONS}" -t "${THREADS}" -d "${DURATION}s" "${url}" > "$output_file" 2>&1
            ;;
        latency_10k)
            wrk2 -c "${CONNECTIONS}" -t "${THREADS}" -d "${DURATION}s" -R 10000 "${url}" > "$output_file" 2>&1
            ;;
        latency_20k)
            wrk2 -c "${CONNECTIONS}" -t "${THREADS}" -d "${DURATION}s" -R 20000 "${url}" > "$output_file" 2>&1
            ;;
    esac
}

# --- main ---

echo "=== Benchmark Runner ==="
echo "Threads: ${THREADS}  Connections: ${CONNECTIONS}  Duration: ${DURATION}s"
echo ""

# Setup
bash "${SCRIPT_DIR}/setup.sh"

# Create output dirs
for scenario in $SCENARIOS; do
    for proxy in $PROXIES; do
        mkdir -p "${OUTPUT_DIR}/${scenario}/${proxy}"
    done
done

# Start backend
start_backend

# Run benchmarks
for scenario in $SCENARIOS; do
    echo ""
    echo "=== Scenario: ${scenario} ==="
    for proxy in $PROXIES; do
        echo ""
        echo "--- Proxy: ${proxy} ---"
        start_proxy "$proxy" "$scenario"

        run_bench "$proxy" "$scenario" "throughput"  "${OUTPUT_DIR}/${scenario}/${proxy}/throughput.txt"
        run_bench "$proxy" "$scenario" "latency_10k" "${OUTPUT_DIR}/${scenario}/${proxy}/latency_10k.txt"
        run_bench "$proxy" "$scenario" "latency_20k" "${OUTPUT_DIR}/${scenario}/${proxy}/latency_20k.txt"

        stop_proxy "$proxy" "$scenario"
    done
done

# Stop backend
stop_backend

# Generate report
echo ""
echo "=== Generating Report ==="
bash "${SCRIPT_DIR}/report.sh" "${OUTPUT_DIR}" "${OUTPUT_DIR}/report.md"

echo ""
echo "=== Done ==="
echo "Results: ${OUTPUT_DIR}/"
echo "Report:  ${OUTPUT_DIR}/report.md"
```

- [ ] **Step 2: Make executable and syntax check**

```bash
chmod +x bench/run.sh
bash -n bench/run.sh
```

Expected: no output (clean syntax)

- [ ] **Step 3: Commit**

```bash
git add bench/run.sh
git commit -m "bench: add main benchmark runner script"
```

---

### Task 8: Final Integration Verification

- [ ] **Step 1: Verify all files exist**

```bash
find bench/ -type f | sort
```

Expected output:

```
bench/configs/haproxy/loadbalance.cfg
bench/configs/haproxy/passthrough.cfg
bench/configs/haproxy/routing.cfg
bench/configs/nginx/loadbalance.conf
bench/configs/nginx/passthrough.conf
bench/configs/nginx/routing.conf
bench/configs/rustproxy/loadbalance.yaml
bench/configs/rustproxy/passthrough.yaml
bench/configs/rustproxy/routing.yaml
bench/report.sh
bench/run.sh
bench/setup.sh
bench/tools/backend.conf
bench/tools/response.txt
```

- [ ] **Step 2: Verify all shell scripts are syntactically valid**

```bash
for f in bench/setup.sh bench/run.sh bench/report.sh; do
    bash -n "$f" && echo "OK: $f"
done
```

Expected: all three print OK

- [ ] **Step 3: Verify response.txt is exactly 256 bytes**

```bash
wc -c bench/tools/response.txt
```

Expected: `256 bench/tools/response.txt`

- [ ] **Step 4: Final commit (if any fixes needed)**

```bash
git add -A bench/
git commit -m "bench: finalize benchmark suite" || echo "Nothing to commit"
```
