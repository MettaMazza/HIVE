#!/bin/bash
# ══════════════════════════════════════════════════════════════════════
#  🐝 HIVE — One-Click Launcher
# ══════════════════════════════════════════════════════════════════════
#
#  This script does EVERYTHING:
#    1. Checks if Docker is installed — installs it if not
#    2. Starts Docker if it's not running
#    3. Builds the HIVE container (first time only)
#    4. Launches HIVE with all mesh services
#    5. Opens HivePortal in your browser
#
#  Usage:
#    chmod +x launch.sh
#    ./launch.sh
#
#  To stop:
#    ./launch.sh stop
#
#  To rebuild (after git pull):
#    ./launch.sh rebuild
#
# ══════════════════════════════════════════════════════════════════════

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

PORTAL_PORT=3035

banner() {
    echo ""
    echo -e "${YELLOW}═══════════════════════════════════════════════════════${NC}"
    echo -e "${YELLOW}  🐝 HIVE — Human Internet Viable Ecosystem${NC}"
    echo -e "${YELLOW}═══════════════════════════════════════════════════════${NC}"
    echo ""
}

log() { echo -e "${GREEN}[HIVE]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; }
info() { echo -e "${BLUE}[INFO]${NC} $1"; }

# ── Handle stop/rebuild commands ────────────────────────────────────
if [ "$1" = "stop" ]; then
    banner
    log "Stopping HIVE..."
    # Kill Flux server if running
    if [ -f /tmp/hive_flux.pid ]; then
        kill "$(cat /tmp/hive_flux.pid)" 2>/dev/null || true
        rm -f /tmp/hive_flux.pid
        log "🎨 Flux server stopped."
    fi
    # Kill Training server if running
    if [ -f /tmp/hive_train.pid ]; then
        kill "$(cat /tmp/hive_train.pid)" 2>/dev/null || true
        rm -f /tmp/hive_train.pid
        log "🧠 Training server stopped."
    fi
    lsof -ti:8491 | xargs kill 2>/dev/null || true
    docker compose down 2>/dev/null || docker-compose down 2>/dev/null || true
    log "✅ HIVE stopped."
    exit 0
fi

if [ "$1" = "rebuild" ]; then
    banner
    log "Rebuilding HIVE from source..."
    # Kill Flux server if running
    lsof -ti:8490 | xargs kill 2>/dev/null || true
    if [ -f /tmp/hive_flux.pid ]; then
        kill "$(cat /tmp/hive_flux.pid)" 2>/dev/null || true
        rm -f /tmp/hive_flux.pid
    fi
    # Kill Training server if running
    lsof -ti:8491 | xargs kill 2>/dev/null || true
    if [ -f /tmp/hive_train.pid ]; then
        kill "$(cat /tmp/hive_train.pid)" 2>/dev/null || true
        rm -f /tmp/hive_train.pid
    fi
    docker compose down 2>/dev/null || true
    # Fall through to the main launch flow (build + start + flux + browser)
fi

if [ "$1" != "rebuild" ]; then
    banner
fi

# ── Step 1: Check/Install Docker ────────────────────────────────────
install_docker() {
    OS="$(uname -s)"
    case "$OS" in
        Darwin)
            log "🍎 macOS detected"
            if ! command -v brew &>/dev/null; then
                log "Installing Homebrew first (required for Docker install)..."
                /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
                eval "$(/opt/homebrew/bin/brew shellenv)" 2>/dev/null || true
            fi
            if command -v brew &>/dev/null; then
                log "Installing Docker via Homebrew..."
                brew install --cask docker
                log "✅ Docker installed. Opening Docker Desktop..."
                open -a Docker
                echo ""
                warn "⏳ Docker Desktop is starting up."
                warn "   Please wait for the whale icon to appear in your menu bar,"
                warn "   then run this script again."
                echo ""
                exit 0
            else
                error "Could not install Homebrew. Install Docker Desktop manually:"
                error "  https://docs.docker.com/desktop/install/mac-install/"
                exit 1
            fi
            ;;
        Linux)
            log "🐧 Linux detected"
            if command -v apt-get &>/dev/null; then
                log "Installing Docker via apt..."
                sudo apt-get update
                sudo apt-get install -y docker.io docker-compose-plugin
                sudo systemctl start docker
                sudo systemctl enable docker
                sudo usermod -aG docker "$USER"
                log "✅ Docker installed."
                warn "You may need to log out and back in for group changes."
                warn "Or run: newgrp docker"
            elif command -v dnf &>/dev/null; then
                log "Installing Docker via dnf..."
                sudo dnf install -y docker docker-compose-plugin
                sudo systemctl start docker
                sudo systemctl enable docker
                sudo usermod -aG docker "$USER"
                log "✅ Docker installed."
            elif command -v pacman &>/dev/null; then
                log "Installing Docker via pacman..."
                sudo pacman -S --noconfirm docker docker-compose
                sudo systemctl start docker
                sudo systemctl enable docker
                sudo usermod -aG docker "$USER"
                log "✅ Docker installed."
            else
                error "Unsupported package manager. Install Docker manually:"
                error "  https://docs.docker.com/engine/install/"
                exit 1
            fi
            ;;
        *)
            error "Unsupported OS: $OS"
            error "Install Docker manually: https://docs.docker.com/get-docker/"
            exit 1
            ;;
    esac
}

if ! command -v docker &>/dev/null; then
    warn "Docker not found on this system."
    echo ""
    read -p "    Install Docker automatically? (y/n) " -n 1 -r
    echo ""
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        install_docker
    else
        error "Docker is required. Install it from https://docs.docker.com/get-docker/"
        exit 1
    fi
fi

log "✅ Docker found: $(docker --version 2>/dev/null | head -1)"

# ── Step 2: Ensure Docker is running ────────────────────────────────
if ! docker info &>/dev/null 2>&1; then
    warn "Docker is installed but not running."

    OS="$(uname -s)"
    if [ "$OS" = "Darwin" ]; then
        log "Starting Docker Desktop..."
        open -a Docker 2>/dev/null || true

        # Wait for Docker to start (up to 60s)
        echo -n "    Waiting for Docker to be ready"
        for i in $(seq 1 60); do
            if docker info &>/dev/null 2>&1; then
                echo ""
                log "✅ Docker is ready."
                break
            fi
            echo -n "."
            sleep 1
        done

        if ! docker info &>/dev/null 2>&1; then
            echo ""
            error "Docker didn't start in time. Please open Docker Desktop manually and try again."
            exit 1
        fi
    else
        log "Starting Docker daemon..."
        sudo systemctl start docker 2>/dev/null || sudo service docker start 2>/dev/null || true
        sleep 2
        if ! docker info &>/dev/null 2>&1; then
            error "Failed to start Docker. Try: sudo systemctl start docker"
            exit 1
        fi
        log "✅ Docker daemon started."
    fi
fi

# ── Step 3: Check if docker compose exists ──────────────────────────
COMPOSE_CMD=""
if docker compose version &>/dev/null 2>&1; then
    COMPOSE_CMD="docker compose"
elif command -v docker-compose &>/dev/null; then
    COMPOSE_CMD="docker-compose"
else
    warn "docker compose not found. Installing..."
    OS="$(uname -s)"
    if [ "$OS" = "Darwin" ]; then
        # Docker Desktop includes compose
        error "Docker Compose should be included with Docker Desktop."
        error "Please reinstall Docker Desktop."
        exit 1
    else
        sudo apt-get install -y docker-compose-plugin 2>/dev/null || \
        sudo dnf install -y docker-compose-plugin 2>/dev/null || \
        pip3 install docker-compose 2>/dev/null || true

        if docker compose version &>/dev/null 2>&1; then
            COMPOSE_CMD="docker compose"
        elif command -v docker-compose &>/dev/null; then
            COMPOSE_CMD="docker-compose"
        else
            error "Could not install docker compose. Install manually."
            exit 1
        fi
    fi
fi

log "✅ Compose: $($COMPOSE_CMD version 2>/dev/null | head -1)"

# ── Step 4: Build & Launch ──────────────────────────────────────────
echo ""
log "🔨 Building HIVE container (this takes ~5 min first time)..."
echo ""

# ── Step 4.5: Start Flux server on HOST (GPU access) ────────────────
# Flux needs MPS/Metal which is only available on the host, not inside Docker.
# This mirrors the Ollama pattern — GPU service on host, HTTP from container.
FLUX_SCRIPT="src/computer/flux_server.py"
if [ -f "$FLUX_SCRIPT" ]; then
    # Find the right Python
    PYTHON_BIN=$(grep HIVE_PYTHON_BIN .env 2>/dev/null | grep -v '^#' | cut -d= -f2- | tr -d '"' || echo "")
    if [ -z "$PYTHON_BIN" ]; then
        PYTHON_BIN="python3"
    fi
    # Kill any existing Flux server (port-based, more reliable than PID files)
    lsof -ti:8490 | xargs kill 2>/dev/null || true
    if [ -f /tmp/hive_flux.pid ]; then
        kill "$(cat /tmp/hive_flux.pid)" 2>/dev/null || true
        rm -f /tmp/hive_flux.pid
    fi
    # Start Flux server in background
    "$PYTHON_BIN" "$FLUX_SCRIPT" &
    echo $! > /tmp/hive_flux.pid
    log "🎨 Flux server starting on http://localhost:8490 (host GPU)"
fi

# ── Step 4.6: Start Training server on HOST (MLX/Metal) ─────────────
# Training needs MLX/Metal for LoRA fine-tuning — runs on the host.
# Docker calls this via http://host.docker.internal:8491/train
TRAIN_SCRIPT="training/train_server.py"
if [ -f "$TRAIN_SCRIPT" ]; then
    # Kill any existing training server
    lsof -ti:8491 | xargs kill 2>/dev/null || true
    if [ -f /tmp/hive_train.pid ]; then
        kill "$(cat /tmp/hive_train.pid)" 2>/dev/null || true
        rm -f /tmp/hive_train.pid
    fi
    # Start training server in background
    "$PYTHON_BIN" "$TRAIN_SCRIPT" &
    echo $! > /tmp/hive_train.pid
    log "🧠 Training server starting on http://localhost:8491 (host MLX/Metal)"
fi

$COMPOSE_CMD up -d --build

echo ""
log "✅ HIVE is running!"
echo ""
echo -e "  ${BOLD}Your mesh network is live:${NC}"
echo ""
echo -e "  ${GREEN}🏠 HivePortal${NC}    → ${BOLD}http://localhost:${PORTAL_PORT}${NC}  ← START HERE"
echo -e "  ${GREEN}🌐 HiveSurface${NC}   → http://localhost:3032"
echo -e "  ${GREEN}💬 HiveChat${NC}      → http://localhost:3034"
echo -e "  ${GREEN}💻 Apis Code${NC}     → http://localhost:3033"
echo -e "  ${GREEN}📖 Apis Book${NC}     → http://localhost:3031"
echo -e "  ${GREEN}👁️  Panopticon${NC}    → http://localhost:3030"
echo ""
echo -e "  ${YELLOW}Commands:${NC}"
echo -e "    ./launch.sh stop     — Stop HIVE"
echo -e "    ./launch.sh rebuild  — Rebuild after updates"
echo -e "    docker logs -f hive-mesh  — View live logs"
echo ""

# ── Step 5: Open browser ───────────────────────────────────────────
sleep 2
URL="http://localhost:${PORTAL_PORT}"
OS="$(uname -s)"
case "$OS" in
    Darwin)  open "$URL" 2>/dev/null ;;
    Linux)   xdg-open "$URL" 2>/dev/null || sensible-browser "$URL" 2>/dev/null ;;
esac

log "🐝 Welcome to the mesh. You are the internet now."
echo ""
