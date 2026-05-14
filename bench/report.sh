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
MODE_LABELS="throughput:Max Throughput latency_10k:Latency @ 10k RPS latency_20k:Latency @ 20k RPS"

extract_metric() {
    local file="$1" metric="$2"
    if [[ ! -f "$file" ]]; then
        echo "N/A"
        return
    fi
    case "$metric" in
        rps)
            grep -oP 'Requests/sec:\s+\K[\d.]+' "$file" 2>/dev/null || echo "N/A"
            ;;
        avg)
            grep -oP 'Latency\s+\K[\d.]+[a-z]+' "$file" 2>/dev/null | head -1 || echo "N/A"
            ;;
        p50)
            grep -oP '50\.000%\s+\K[\d.]+[a-z]+' "$file" 2>/dev/null || echo "N/A"
            ;;
        p90)
            grep -oP '90\.000%\s+\K[\d.]+[a-z]+' "$file" 2>/dev/null || echo "N/A"
            ;;
        p99)
            grep -oP '99\.000%\s+\K[\d.]+[a-z]+' "$file" 2>/dev/null || echo "N/A"
            ;;
        p999)
            grep -oP '99\.900%\s+\K[\d.]+[a-z]+' "$file" 2>/dev/null || echo "N/A"
            ;;
        transfer)
            grep -oP 'Transfer/sec:\s+\K[\d.]+[A-Z]+B' "$file" 2>/dev/null || echo "N/A"
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
