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
    docker compose down 2>/dev/null || docker-compose down 2>/dev/null || true
    log "✅ HIVE stopped."
    exit 0
fi

if [ "$1" = "rebuild" ]; then
    banner
    log "Rebuilding HIVE from source..."
    docker compose down 2>/dev/null || true
    docker compose build --no-cache
    log "✅ Rebuild complete. Run ./launch.sh to start."
    exit 0
fi

banner

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
