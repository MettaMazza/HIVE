# ══════════════════════════════════════════════════════════════════════
#  🐝 HIVE — Multi-Stage Docker Build
# ══════════════════════════════════════════════════════════════════════
# This Dockerfile builds HIVE from source and packages it with Ollama
# for a fully self-contained, zero-dependency deployment.
#
# Usage:
#   docker build -t hive .
#   docker run -p 3030-3035:3030-3035 -p 8421:8421 -p 8480:8480 hive
#
# Or just run: ./launch.sh
# ══════════════════════════════════════════════════════════════════════

# ── Stage 1: Build HIVE from source ─────────────────────────────────
FROM rust:1.82-bookworm AS builder

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

# Install Ollama
RUN curl -fsSL https://ollama.com/install.sh | sh

# Create hive user for security
RUN useradd -m -s /bin/bash hive
WORKDIR /home/hive

# Copy HIVE binary
COPY --from=builder /build/target/release/HIVE /usr/local/bin/hive

# Copy configuration files
COPY .env.example .env
COPY prompts/ prompts/
COPY README.md .

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

# Start Ollama in background
echo "🤖 Starting Ollama inference engine..."
ollama serve &
OLLAMA_PID=$!
sleep 3

# Pull default model if not present
if ! ollama list 2>/dev/null | grep -q "qwen"; then
    echo "📦 Pulling default AI model (this only happens once)..."
    ollama pull qwen2.5:7b || echo "⚠️  Model pull failed — will retry on first use"
fi

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
