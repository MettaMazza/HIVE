# ══════════════════════════════════════════════════════════════════════
#  🐝 HIVE — Multi-Stage Docker Build
# ══════════════════════════════════════════════════════════════════════
# Uses your HOST's Ollama (not bundled) — Metal/GPU acceleration works
# natively on your machine, not inside a container.
#
# Usage:
#   docker build -t hive .
#   docker run -p 3030-3035:3030-3035 hive
#
# Or just run: ./launch.sh
# ══════════════════════════════════════════════════════════════════════

# ── Stage 1: Build HIVE from source ─────────────────────────────────
FROM rust:1.85-bookworm AS builder

WORKDIR /build

# Cache dependencies first (faster rebuilds)
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release 2>/dev/null || true && \
    rm -rf src

# Copy full source and build
COPY . .
RUN cargo build --release && \
    strip target/release/HIVE

# ── Stage 2: Runtime ────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    python3 \
    && rm -rf /var/lib/apt/lists/*

# Create hive user for security
RUN useradd -m -s /bin/bash hive
WORKDIR /home/hive

# Copy HIVE binary
COPY --from=builder /build/target/release/HIVE /usr/local/bin/hive

# Copy configuration files
COPY .env.example .env
COPY README.md .
COPY persona.toml.example .hive/persona.toml

# Create required directories
RUN mkdir -p memory .hive && \
    chown -R hive:hive /home/hive

# ── Entrypoint script ──────────────────────────────────────────────
COPY <<'ENTRYPOINT' /usr/local/bin/start-hive.sh
#!/bin/bash
set -e

echo "🐝 ═══════════════════════════════════════════════════════"
echo "🐝  HIVE — Human Internet Viable Ecosystem"
echo "🐝  Starting all services..."
echo "🐝 ═══════════════════════════════════════════════════════"

# Check Ollama connectivity (uses host Ollama via OLLAMA_BASE_URL)
OLLAMA_URL="${OLLAMA_BASE_URL:-http://host.docker.internal:11434}"
echo "🤖 Connecting to Ollama at ${OLLAMA_URL}..."
for i in $(seq 1 10); do
    if curl -sf "${OLLAMA_URL}/api/tags" >/dev/null 2>&1; then
        echo "🤖 ✅ Ollama connected!"
        break
    fi
    if [ "$i" -eq 10 ]; then
        echo "⚠️  Could not reach Ollama at ${OLLAMA_URL}"
        echo "    Make sure Ollama is running on your host machine."
        echo "    HIVE will start anyway — AI features won't work until Ollama is available."
    fi
    sleep 1
done

echo ""
echo "🌐 Starting HIVE mesh network..."
echo ""
echo "  📡 Panopticon    → http://localhost:3030"
echo "  📖 Apis Book     → http://localhost:3031"
echo "  🌐 HiveSurface   → http://localhost:3032"
echo "  💻 Apis Code     → http://localhost:3033"
echo "  💬 HiveChat      → http://localhost:3034"
echo "  🏠 HivePortal    → http://localhost:3035  ← OPEN THIS"
echo ""
echo "🐝 ═══════════════════════════════════════════════════════"

# Disable auto-open inside container (no browser)
export HIVE_AUTO_OPEN=false

# Point at host Ollama
export OLLAMA_BASE_URL="${OLLAMA_URL}"

# Run HIVE
exec /usr/local/bin/hive
ENTRYPOINT
RUN chmod +x /usr/local/bin/start-hive.sh

# Expose all ports
EXPOSE 3030 3031 3032 3033 3034 3035 8421 8422 8480

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:3035/api/status || exit 1

USER hive
ENTRYPOINT ["/usr/local/bin/start-hive.sh"]
