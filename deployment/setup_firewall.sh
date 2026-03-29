#!/usr/bin/env bash
# setup_firewall.sh — Isolate the HIVE container from your LAN
# Supports: Linux (iptables), macOS (pf)
# Usage:
#   ./setup_firewall.sh            — auto-detect LAN, apply rules
#   ./setup_firewall.sh --dry-run  — show what would be done, change nothing
#   ./setup_firewall.sh --remove   — remove previously applied rules
set -euo pipefail

# ── Config ────────────────────────────────────────────────────────────────────
HIVE_SUBNET="172.28.0.0/16"          # must match docker-compose.yml ipam subnet
COMPOSE_PROJECT="hive"               # docker compose project name (folder name by default)
PF_ANCHOR="hive_lan_block"           # macOS pf anchor name
IPTABLES_COMMENT="hive-lan-block"    # iptables rule comment for identification

# ── Helpers ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; YELLOW='\033[1;33m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'; NC='\033[0m'
info()    { echo -e "${CYAN}[info]${NC}  $*"; }
success() { echo -e "${GREEN}[ok]${NC}    $*"; }
warn()    { echo -e "${YELLOW}[warn]${NC}  $*"; }
error()   { echo -e "${RED}[error]${NC} $*" >&2; }
die()     { error "$*"; exit 1; }

DRY_RUN=false
REMOVE=false
for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=true ;;
        --remove)  REMOVE=true ;;
        --help|-h)
            echo "Usage: $0 [--dry-run|--remove]"
            echo "  (no flags)  Detect LAN and apply firewall rules"
            echo "  --dry-run   Show commands without executing them"
            echo "  --remove    Remove previously applied rules"
            exit 0 ;;
        *) die "Unknown argument: $arg" ;;
    esac
done

run() {
    # Print and optionally execute a command
    echo -e "  ${CYAN}\$${NC} $*"
    if ! $DRY_RUN; then
        "$@"
    fi
}

# ── OS detection ──────────────────────────────────────────────────────────────
OS="$(uname -s)"
case "$OS" in
    Linux)  PLATFORM="linux" ;;
    Darwin) PLATFORM="macos" ;;
    *)      die "Unsupported OS: $OS (only Linux and macOS supported)" ;;
esac
info "Detected platform: $PLATFORM"

# ── Privilege check ───────────────────────────────────────────────────────────
if [[ "$EUID" -ne 0 ]]; then
    if command -v sudo &>/dev/null; then
        info "Re-running with sudo..."
        exec sudo bash "$0" "$@"
    else
        die "This script must be run as root (sudo not found)"
    fi
fi

# ── LAN subnet detection ──────────────────────────────────────────────────────
detect_lan_subnets() {
    local subnets=()

    if [[ "$PLATFORM" == "linux" ]]; then
        # Parse 'ip route' for directly connected (non-default) routes
        while IFS= read -r line; do
            # Lines like: 192.168.1.0/24 dev eth0 proto kernel ...
            if [[ "$line" =~ ^([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+/[0-9]+)[[:space:]] ]]; then
                subnet="${BASH_REMATCH[1]}"
                # Skip loopback, Docker default bridge, and our HIVE subnet
                [[ "$subnet" == 127.* ]]        && continue
                [[ "$subnet" == 172.17.* ]]     && continue
                [[ "$subnet" == "$HIVE_SUBNET" ]] && continue
                subnets+=("$subnet")
            fi
        done < <(ip route 2>/dev/null | grep -v default)

    elif [[ "$PLATFORM" == "macos" ]]; then
        # Parse 'netstat -rn' for inet routes
        while IFS= read -r line; do
            # Lines like: 192.168.1.0/24   link#...
            if [[ "$line" =~ ^([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+/[0-9]+) ]]; then
                subnet="${BASH_REMATCH[1]}"
                [[ "$subnet" == 127.* ]]        && continue
                [[ "$subnet" == 169.254.* ]]    && continue
                [[ "$subnet" == "$HIVE_SUBNET" ]] && continue
                subnets+=("$subnet")
            fi
        done < <(netstat -rn -f inet 2>/dev/null | grep -v default | grep -v '^Destination')
    fi

    # Deduplicate
    printf '%s\n' "${subnets[@]}" | sort -u
}

info "Detecting local network subnets..."
mapfile -t LAN_SUBNETS < <(detect_lan_subnets)

if [[ ${#LAN_SUBNETS[@]} -eq 0 ]]; then
    warn "No LAN subnets detected automatically."
    warn "Is your network interface up? You can specify one manually:"
    warn "  Edit this script and set LAN_SUBNETS=(\"192.168.1.0/24\")"
    die "Cannot continue without a LAN subnet"
fi

echo ""
info "Detected LAN subnets:"
for s in "${LAN_SUBNETS[@]}"; do
    echo "    $s"
done
echo ""
info "HIVE container subnet: $HIVE_SUBNET"
echo ""

# ── Verify Docker network exists (warn if not) ────────────────────────────────
check_docker_network() {
    if command -v docker &>/dev/null; then
        local net_name="${COMPOSE_PROJECT}_hive-net"
        if docker network ls --format '{{.Name}}' 2>/dev/null | grep -q "^${net_name}$"; then
            success "Docker network '${net_name}' found"
        else
            warn "Docker network '${net_name}' not found yet."
            warn "Run 'docker compose up' first, or the rules will still apply"
            warn "when it's created (iptables rules are subnet-based, not network-name-based)."
        fi
    else
        warn "docker not found in PATH — skipping network check"
    fi
}
check_docker_network
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# LINUX — iptables
# ══════════════════════════════════════════════════════════════════════════════
apply_linux() {
    # Check for DOCKER-USER chain (requires Docker 17.06+)
    if ! iptables -L DOCKER-USER &>/dev/null; then
        warn "DOCKER-USER chain not found. Start Docker first, then re-run this script."
        die "DOCKER-USER chain missing"
    fi

    for lan in "${LAN_SUBNETS[@]}"; do
        info "Blocking $HIVE_SUBNET → $lan in DOCKER-USER chain..."
        run iptables -I DOCKER-USER \
            -s "$HIVE_SUBNET" \
            -d "$lan" \
            -m comment --comment "$IPTABLES_COMMENT" \
            -j DROP
    done

    # Verify
    echo ""
    info "Current DOCKER-USER rules matching '$IPTABLES_COMMENT':"
    iptables -L DOCKER-USER -n --line-numbers | grep "$IPTABLES_COMMENT" || true

    # Persist
    persist_linux
}

remove_linux() {
    info "Removing HIVE iptables rules (comment: $IPTABLES_COMMENT)..."
    local removed=0
    # Loop because there may be multiple rules (one per LAN subnet)
    # iptables rule numbers shift after each deletion so we delete by matching
    while iptables -L DOCKER-USER -n --line-numbers 2>/dev/null | grep -q "$IPTABLES_COMMENT"; do
        local linenum
        linenum=$(iptables -L DOCKER-USER -n --line-numbers | grep "$IPTABLES_COMMENT" | head -1 | awk '{print $1}')
        run iptables -D DOCKER-USER "$linenum"
        removed=$((removed + 1))
    done
    if [[ $removed -eq 0 ]]; then
        warn "No rules with comment '$IPTABLES_COMMENT' found — nothing to remove"
    else
        success "Removed $removed rule(s)"
    fi
    # Overwrite saved rules and remove the boot-restore unit
    _persist_save_rules
    _persist_remove_systemd_unit
}

SYSTEMD_UNIT="iptables-restore-hive.service"
RULES_FILE="/etc/iptables/rules.v4"

persist_linux() {
    echo ""
    if command -v netfilter-persistent &>/dev/null; then
        info "Persisting rules with netfilter-persistent..."
        run netfilter-persistent save
        success "Rules will survive reboots"
    elif command -v iptables-save &>/dev/null; then
        _persist_save_rules
        _persist_install_systemd_unit
    else
        warn "Could not persist rules automatically — they will be lost on reboot."
        warn "Install 'iptables-persistent' to fix this: sudo apt install iptables-persistent"
    fi
}

_persist_save_rules() {
    mkdir -p "$(dirname "$RULES_FILE")"
    info "Saving rules to $RULES_FILE..."
    if ! $DRY_RUN; then
        iptables-save > "$RULES_FILE"
        success "Rules saved to $RULES_FILE"
    else
        echo "  (dry-run) iptables-save > $RULES_FILE"
    fi
}

_persist_install_systemd_unit() {
    local unit_file="/etc/systemd/system/${SYSTEMD_UNIT}"

    # Skip if systemd is not running (e.g. containers, WSL1)
    if ! command -v systemctl &>/dev/null || ! systemctl is-system-running &>/dev/null 2>&1 | grep -qE 'running|degraded'; then
        warn "systemd not available — rules saved to $RULES_FILE but will NOT auto-restore on reboot."
        return
    fi

    info "Installing systemd unit $SYSTEMD_UNIT..."
    if ! $DRY_RUN; then
        cat > "$unit_file" << EOF
[Unit]
Description=Restore HIVE iptables LAN-isolation rules
# Must run before Docker so DOCKER-USER chain exists when we restore into it
Before=docker.service
Wants=network-pre.target
After=network-pre.target

[Service]
Type=oneshot
ExecStart=/sbin/iptables-restore --noflush ${RULES_FILE}
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOF
        systemctl daemon-reload
        systemctl enable "$SYSTEMD_UNIT"
        success "Systemd unit installed and enabled — rules will survive reboots"
    else
        echo "  (dry-run) would write $unit_file with iptables-restore --noflush $RULES_FILE"
        echo "  (dry-run) systemctl daemon-reload"
        echo "  (dry-run) systemctl enable $SYSTEMD_UNIT"
    fi
}

_persist_remove_systemd_unit() {
    local unit_file="/etc/systemd/system/${SYSTEMD_UNIT}"
    if [[ ! -f "$unit_file" ]]; then
        return  # nothing to remove
    fi
    info "Disabling and removing systemd unit $SYSTEMD_UNIT..."
    if ! $DRY_RUN; then
        systemctl disable "$SYSTEMD_UNIT" 2>/dev/null || true
        rm -f "$unit_file"
        systemctl daemon-reload
        success "Systemd unit removed"
    else
        echo "  (dry-run) systemctl disable $SYSTEMD_UNIT"
        echo "  (dry-run) rm $unit_file"
        echo "  (dry-run) systemctl daemon-reload"
    fi
}

# ══════════════════════════════════════════════════════════════════════════════
# MACOS — pf (packet filter)
# ══════════════════════════════════════════════════════════════════════════════
apply_macos() {
    local anchor_file="/etc/pf.anchors/${PF_ANCHOR}"
    local pf_conf="/etc/pf.conf"

    info "Writing pf anchor rules to $anchor_file..."
    if ! $DRY_RUN; then
        {
            echo "# HIVE container LAN isolation — generated by setup_firewall.sh"
            echo "# Remove with: $0 --remove"
            for lan in "${LAN_SUBNETS[@]}"; do
                echo "block out quick from $HIVE_SUBNET to $lan"
            done
        } > "$anchor_file"
        cat "$anchor_file"
    else
        echo "  (dry-run) would write to $anchor_file:"
        for lan in "${LAN_SUBNETS[@]}"; do
            echo "    block out quick from $HIVE_SUBNET to $lan"
        done
    fi

    # Add anchor reference to pf.conf if not already present
    if ! grep -q "$PF_ANCHOR" "$pf_conf" 2>/dev/null; then
        info "Adding anchor reference to $pf_conf..."
        run bash -c "echo 'anchor \"${PF_ANCHOR}\"' >> $pf_conf"
        run bash -c "echo 'load anchor \"${PF_ANCHOR}\" from \"${anchor_file}\"' >> $pf_conf"
    else
        info "Anchor reference already in $pf_conf — skipping"
    fi

    # Reload pf
    info "Reloading pf..."
    run pfctl -f "$pf_conf" -e 2>/dev/null || run pfctl -f "$pf_conf"
    success "pf rules applied and will persist across reboots (pf.conf is permanent)"
}

remove_macos() {
    local anchor_file="/etc/pf.anchors/${PF_ANCHOR}"
    local pf_conf="/etc/pf.conf"

    info "Removing pf anchor file: $anchor_file"
    if [[ -f "$anchor_file" ]]; then
        run rm "$anchor_file"
    else
        warn "Anchor file not found: $anchor_file"
    fi

    info "Removing anchor reference from $pf_conf..."
    if grep -q "$PF_ANCHOR" "$pf_conf" 2>/dev/null; then
        if ! $DRY_RUN; then
            sed -i '' "/$PF_ANCHOR/d" "$pf_conf"
        else
            echo "  (dry-run) sed -i '' '/$PF_ANCHOR/d' $pf_conf"
        fi
    else
        warn "No anchor reference found in $pf_conf"
    fi

    info "Reloading pf..."
    run pfctl -f "$pf_conf" 2>/dev/null || true
    success "pf rules removed"
}

# ══════════════════════════════════════════════════════════════════════════════
# Verification
# ══════════════════════════════════════════════════════════════════════════════
verify() {
    echo ""
    info "=== Verification ==="
    if $DRY_RUN; then
        warn "Dry-run mode — skipping live verification"
        return
    fi

    local test_image="alpine"
    if ! docker image inspect "$test_image" &>/dev/null; then
        info "Pulling $test_image for verification..."
        docker pull "$test_image" -q
    fi

    local net_name="${COMPOSE_PROJECT}_hive-net"
    local net_flag="--network $net_name"

    # Fallback: use default bridge if hive-net doesn't exist yet
    if ! docker network ls --format '{{.Name}}' 2>/dev/null | grep -q "^${net_name}$"; then
        warn "hive-net not up yet — verifying using host network as approximation"
        net_flag=""
    fi

    for lan in "${LAN_SUBNETS[@]}"; do
        # Extract a host IP from the subnet (the .1 address = router)
        local gw="${lan%.*}.1"
        info "Testing LAN access to $gw (should be BLOCKED)..."
        # shellcheck disable=SC2086
        if docker run --rm $net_flag "$test_image" \
            ping -c1 -W2 "$gw" &>/dev/null 2>&1; then
            warn "  WARN: $gw is still reachable — rule may not have applied"
        else
            success "  $gw is unreachable — LAN isolation working"
        fi
    done

    info "Testing internet access (should be REACHABLE)..."
    # shellcheck disable=SC2086
    if docker run --rm $net_flag "$test_image" \
        wget -qO- --timeout=5 https://icanhazip.com &>/dev/null 2>&1; then
        success "  Internet reachable — HIVE can reach Discord and web search"
    else
        warn "  Internet appears unreachable — check your Docker network config"
    fi
}

# ══════════════════════════════════════════════════════════════════════════════
# Main
# ══════════════════════════════════════════════════════════════════════════════
$DRY_RUN && warn "=== DRY-RUN MODE — no changes will be made ==="
echo ""

if $REMOVE; then
    info "=== Removing HIVE firewall rules ==="
    [[ "$PLATFORM" == "linux" ]] && remove_linux
    [[ "$PLATFORM" == "macos" ]] && remove_macos
    echo ""
    success "Done. HIVE container can now reach your LAN (rules removed)."
else
    info "=== Applying HIVE firewall rules ==="
    [[ "$PLATFORM" == "linux" ]] && apply_linux
    [[ "$PLATFORM" == "macos" ]] && apply_macos
    verify
    echo ""
    success "Done."
    echo ""
    echo -e "  Rules applied  : ${GREEN}HIVE subnet ($HIVE_SUBNET) → LAN: DROP${NC}"
    echo -e "  Internet access: ${GREEN}allowed${NC}"
    echo ""
    echo "  To remove these rules later:"
    echo "    sudo $0 --remove"
fi
