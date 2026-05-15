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
            awk '/Requests\/sec:/ { print $2; found=1; exit } END { if (!found) print "N/A" }' "$file"
            ;;
        avg)
            awk '/^[[:space:]]*Latency[[:space:]]/ { print $2; found=1; exit } END { if (!found) print "N/A" }' "$file"
            ;;
        p50)
            awk '$1 == "50.000%" { print $2; found=1; exit } END { if (!found) print "N/A" }' "$file"
            ;;
        p90)
            awk '$1 == "90.000%" { print $2; found=1; exit } END { if (!found) print "N/A" }' "$file"
            ;;
        p99)
            awk '$1 == "99.000%" { print $2; found=1; exit } END { if (!found) print "N/A" }' "$file"
            ;;
        p999)
            awk '$1 == "99.900%" { print $2; found=1; exit } END { if (!found) print "N/A" }' "$file"
            ;;
        transfer)
            awk '/Transfer\/sec:/ { print $2; found=1; exit } END { if (!found) print "N/A" }' "$file"
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
