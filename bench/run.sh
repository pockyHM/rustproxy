#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUTPUT_DIR="${PROJECT_ROOT}/bench/results"
THREADS=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
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
            rm -f /tmp/bench_haproxy.pid
            ;;
        rustproxy)
            [[ -f /tmp/bench_rustproxy.pid ]] && kill "$(cat /tmp/bench_rustproxy.pid)" 2>/dev/null || true
            rm -f /tmp/bench_rustproxy.pid
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
