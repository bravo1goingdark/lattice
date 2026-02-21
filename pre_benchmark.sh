#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────
# Configuration
# ─────────────────────────────────────────────

BENCH_CORE=3
BENCH_NAME="analyzer"
WARM_UP=5
MEASURE=15
USE_PERF=1

# ─────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────

info()  { echo -e "\033[1;34m[INFO]\033[0m  $*"; }
ok()    { echo -e "\033[1;32m[ OK ]\033[0m  $*"; }
warn()  { echo -e "\033[1;33m[WARN]\033[0m  $*"; }
die()   { echo -e "\033[1;31m[ERR ]\033[0m  $*"; exit 1; }
hr()    { echo "-----------------------------------------------------"; }

require_root() {
    if [[ $EUID -ne 0 ]]; then
        warn "Some operations require sudo privileges."
    fi
}

check_cpu_online() {
    local online
    online=$(cat /sys/devices/system/cpu/online)
    if ! grep -q "$BENCH_CORE" <<< "$online"; then
        die "CPU core ${BENCH_CORE} is not online. Online CPUs: $online"
    fi
}

lock_intel_pstate() {
    if [[ -f /sys/devices/system/cpu/intel_pstate/min_perf_pct ]]; then
        info "Locking Intel P-State to 100%"
        echo 100 | sudo tee /sys/devices/system/cpu/intel_pstate/min_perf_pct > /dev/null
        echo 100 | sudo tee /sys/devices/system/cpu/intel_pstate/max_perf_pct > /dev/null
        ok "Intel P-State locked"
    else
        info "Using legacy cpufreq governor"
        echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor > /dev/null || true
    fi
}

restore_intel_pstate() {
    if [[ -f /sys/devices/system/cpu/intel_pstate/min_perf_pct ]]; then
        echo 0 | sudo tee /sys/devices/system/cpu/intel_pstate/min_perf_pct > /dev/null
        echo 100 | sudo tee /sys/devices/system/cpu/intel_pstate/max_perf_pct > /dev/null
        ok "Intel P-State restored"
    fi
}

disable_turbo() {
    if [[ -f /sys/devices/system/cpu/intel_pstate/no_turbo ]]; then
        info "Disabling Turbo Boost"
        echo 1 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo > /dev/null
        ok "Turbo disabled"
    fi
}

restore_turbo() {
    if [[ -f /sys/devices/system/cpu/intel_pstate/no_turbo ]]; then
        echo 0 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo > /dev/null
        ok "Turbo restored"
    fi
}

drop_caches() {
    sync
    echo 3 | sudo tee /proc/sys/vm/drop_caches > /dev/null
    ok "Filesystem cache dropped"
}

run_perf() {
    perf stat -e \
        cycles,instructions,cache-misses,branches,branch-misses \
        taskset -c "${BENCH_CORE}" "$@"
}

# ─────────────────────────────────────────────
# Setup
# ─────────────────────────────────────────────

do_setup() {
    hr
    info "Production benchmark setup"
    hr

    check_cpu_online

    info "Pinning to CPU core ${BENCH_CORE}"

    lock_intel_pstate
    disable_turbo

    echo 0 | sudo tee /proc/sys/kernel/randomize_va_space > /dev/null
    ok "ASLR disabled"

    drop_caches

    sudo systemctl stop unattended-upgrades 2>/dev/null || true
    sudo systemctl stop apt-daily.timer 2>/dev/null || true
    sudo systemctl stop apt-daily-upgrade.timer 2>/dev/null || true

    hr
    ok "Environment ready"
    hr
}

# ─────────────────────────────────────────────
# Restore
# ─────────────────────────────────────────────

do_restore() {
    hr
    info "Restoring system"
    hr

    restore_intel_pstate
    restore_turbo

    echo 2 | sudo tee /proc/sys/kernel/randomize_va_space > /dev/null
    ok "ASLR restored"

    sudo systemctl start apt-daily.timer 2>/dev/null || true
    sudo systemctl start apt-daily-upgrade.timer 2>/dev/null || true
    sudo systemctl start unattended-upgrades 2>/dev/null || true

    hr
    ok "System restored"
    hr
}

# ─────────────────────────────────────────────
# Criterion
# ─────────────────────────────────────────────

do_criterion() {
    hr
    info "Running Criterion on core ${BENCH_CORE}"
    hr

    if [[ "${USE_PERF}" == "1" ]]; then
        run_perf cargo bench --bench "${BENCH_NAME}" -- \
            --warm-up-time "${WARM_UP}" \
            --measurement-time "${MEASURE}" \
            --save-baseline main
    else
        taskset -c "${BENCH_CORE}" cargo bench --bench "${BENCH_NAME}" -- \
            --warm-up-time "${WARM_UP}" \
            --measurement-time "${MEASURE}" \
            --save-baseline main
    fi

    hr
}

# ─────────────────────────────────────────────
# Wiki Bench
# ─────────────────────────────────────────────

do_wiki() {
    local file="${1:-}"
    local mode="${2:-pipeline}"
    local field="${3:-body}"

    [[ -z "$file" ]] && die "No wiki file specified"
    [[ -f "$file" ]] || die "File not found: $file"

    cargo build --release --bin wiki_bench

    hr
    info "wiki_bench -- mode: $mode -- field: $field"
    hr

    if [[ "${USE_PERF}" == "1" ]]; then
        run_perf ./target/release/wiki_bench "$file" "$mode" "$field"
    else
        taskset -c "${BENCH_CORE}" \
            ./target/release/wiki_bench "$file" "$mode" "$field"
    fi

    hr
    ok "wiki_bench done"
    hr
}

# ─────────────────────────────────────────────
# Entrypoint
# ─────────────────────────────────────────────

case "${1:-}" in
    setup)
        do_setup
        ;;
    restore)
        do_restore
        ;;
    run:criterion)
        do_setup
        do_criterion
        do_restore
        ;;
    run:wiki)
        do_setup
        do_wiki "${2:-}" "${3:-pipeline}" "${4:-body}"
        do_restore
        ;;
    *)
        echo ""
        echo "Usage:"
        echo "  $0 setup"
        echo "  $0 restore"
        echo "  $0 run:criterion"
        echo "  $0 run:wiki <file> <mode> [field]"
        echo ""
        exit 1
        ;;
esac