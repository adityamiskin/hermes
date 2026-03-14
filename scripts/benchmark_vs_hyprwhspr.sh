#!/usr/bin/env bash
set -euo pipefail

duration="${1:-20}"
interval="${2:-1}"

if ! [[ "$duration" =~ ^[0-9]+$ ]] || ! [[ "$interval" =~ ^[0-9]+$ ]] || [[ "$interval" -le 0 ]]; then
  echo "Usage: $0 [duration_seconds] [sample_interval_seconds]"
  echo "Example: $0 30 1"
  exit 2
fi

samples=$(( duration / interval ))
if [[ "$samples" -lt 1 ]]; then
  samples=1
fi

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1"
    exit 1
  fi
}

require_cmd systemctl
require_cmd ps
require_cmd awk
require_cmd date

service_exists() {
  systemctl --user list-unit-files --type=service | awk '{print $1}' | grep -qx "$1"
}

main_pid() {
  systemctl --user show "$1" -p MainPID --value
}

restart_ms() {
  local svc="$1"
  local t0 t1
  t0="$(date +%s%N)"
  systemctl --user restart "$svc"
  t1="$(date +%s%N)"
  echo $(( (t1 - t0) / 1000000 ))
}

cli_latency_ms() {
  local cmd="$1"
  local t0 t1
  t0="$(date +%s%N)"
  bash -lc "$cmd" >/dev/null
  t1="$(date +%s%N)"
  echo $(( (t1 - t0) / 1000000 ))
}

sample_process() {
  local pid="$1"
  local i rss cpu
  local rss_sum=0
  local cpu_sum=0
  local rss_peak=0
  local ok=0

  for ((i = 0; i < samples; i++)); do
    if read -r rss cpu < <(ps -p "$pid" -o rss=,%cpu= 2>/dev/null); then
      rss="${rss// /}"
      cpu="${cpu// /}"
      rss_sum=$(( rss_sum + rss ))
      cpu_sum="$(awk -v a="$cpu_sum" -v b="$cpu" 'BEGIN { printf "%.6f", a + b }')"
      if (( rss > rss_peak )); then
        rss_peak="$rss"
      fi
      ok=$((ok + 1))
    fi
    sleep "$interval"
  done

  if (( ok == 0 )); then
    echo "0,0,0"
    return
  fi

  awk -v rss_sum="$rss_sum" -v cpu_sum="$cpu_sum" -v rss_peak="$rss_peak" -v n="$ok" 'BEGIN {
    printf "%.2f,%.2f,%.2f\n", rss_sum / n / 1024.0, cpu_sum / n, rss_peak / 1024.0
  }'
}

print_header() {
  echo "Benchmark duration: ${duration}s, interval: ${interval}s (${samples} samples)"
  echo
  printf "%-14s | %10s | %11s | %12s | %11s | %12s\n" "Service" "Restart ms" "CLI ms" "Avg RSS (MB)" "Peak RSS MB" "Avg CPU %"
  printf "%-14s-+-%10s-+-%11s-+-%12s-+-%11s-+-%12s\n" "--------------" "----------" "-----------" "------------" "-----------" "------------"
}

measure_one() {
  local label="$1"
  local svc="$2"
  local cli_cmd="$3"
  local restart cli pid stats avg_rss avg_cpu peak_rss

  if ! service_exists "$svc"; then
    echo "N/A,N/A,N/A,N/A,N/A"
    return
  fi

  systemctl --user start "$svc"
  restart="$(restart_ms "$svc")"
  cli="$(cli_latency_ms "$cli_cmd")"
  pid="$(main_pid "$svc")"
  if [[ -z "$pid" || "$pid" == "0" ]]; then
    echo "${restart},${cli},N/A,N/A,N/A"
    return
  fi

  stats="$(sample_process "$pid")"
  IFS=',' read -r avg_rss avg_cpu peak_rss <<<"$stats"
  echo "${restart},${cli},${avg_rss},${peak_rss},${avg_cpu}"
}

print_row() {
  local label="$1"
  local metrics="$2"
  local restart cli avg_rss peak_rss avg_cpu
  IFS=',' read -r restart cli avg_rss peak_rss avg_cpu <<<"$metrics"
  printf "%-14s | %10s | %11s | %12s | %11s | %12s\n" "$label" "$restart" "$cli" "$avg_rss" "$peak_rss" "$avg_cpu"
}

print_delta() {
  local h_metrics="$1"
  local p_metrics="$2"
  local h_restart h_cli h_rss h_peak h_cpu
  local p_restart p_cli p_rss p_peak p_cpu
  IFS=',' read -r h_restart h_cli h_rss h_peak h_cpu <<<"$h_metrics"
  IFS=',' read -r p_restart p_cli p_rss p_peak p_cpu <<<"$p_metrics"

  if [[ "$h_rss" == "N/A" || "$p_rss" == "N/A" ]]; then
    echo
    echo "Delta summary: unavailable (missing service or sampling failed)."
    return
  fi

  echo
  echo "Delta summary (Hermes vs Hyprwhspr):"
  awk -v h_r="$h_rss" -v p_r="$p_rss" -v h_p="$h_peak" -v p_p="$p_peak" -v h_c="$h_cpu" -v p_c="$p_cpu" 'BEGIN {
    printf "  Avg RSS:  %.2f MB (%+.1f%%)\n", h_r - p_r, ((h_r - p_r) / p_r) * 100.0
    printf "  Peak RSS: %.2f MB (%+.1f%%)\n", h_p - p_p, ((h_p - p_p) / p_p) * 100.0
    printf "  Avg CPU:  %.2f %% (%+.1f%%)\n", h_c - p_c, ((h_c - p_c) / p_c) * 100.0
  }'
}

hermes_metrics="$(measure_one "Hermes" "hermes.service" "hermes status")"
hypr_metrics="$(measure_one "Hyprwhspr" "hyprwhspr.service" "hyprwhspr status")"

print_header
print_row "Hermes" "$hermes_metrics"
print_row "Hyprwhspr" "$hypr_metrics"
print_delta "$hermes_metrics" "$hypr_metrics"
