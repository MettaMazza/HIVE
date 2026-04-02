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
FROM rust:latest AS builder

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
FROM debian:trixie-slim AS runtime

# ── Layer 1: Python + training stack (STABLE — rarely changes) ──────
# This layer is separated so that adding system tools later
# does NOT invalidate the expensive ~2GB PyTorch download.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl libssl3 python3 python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Copy ONLY requirements files first — this layer only changes when
# dependencies change, so pip install stays cached across rebuilds.
COPY training/requirements*.txt training/
RUN pip3 install --no-cache-dir --break-system-packages -r training/requirements_torch.txt

# NOW copy training scripts — changing .py files no longer busts pip cache
COPY training/*.py training/

# Flux dependencies (cached in this layer — scripts copied later after WORKDIR)
RUN pip3 install --no-cache-dir --break-system-packages diffusers transformers accelerate sentencepiece protobuf torchvision

# TTS dependencies (Kokoro voice synthesis)
RUN pip3 install --no-cache-dir --break-system-packages soundfile kokoro-onnx

# ── Layer 2: System tools (can be modified without busting pip cache) ─
RUN apt-get update && apt-get install -y --no-install-recommends \
    bash git findutils grep tar lsof procps \
    build-essential pkg-config libssl-dev libsndfile1 \
    chromium \
    && curl -fsSL https://pkg.cloudflare.com/cloudflare-main.gpg \
       -o /usr/share/keyrings/cloudflare-main.gpg \
    && echo "deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main" \
       > /etc/apt/sources.list.d/cloudflared.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends cloudflared \
    && rm -rf /var/lib/apt/lists/*

# Copy Rust toolchain from builder (Debian repos have 1.85; we need 1.88+)
COPY --from=builder /usr/local/cargo /usr/local/cargo
COPY --from=builder /usr/local/rustup /usr/local/rustup
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

# Create hive user for security
RUN useradd -m -s /bin/bash hive

# (chown of cargo/rustup happens after all COPY steps below)

WORKDIR /home/hive

# Copy HIVE binary (both to PATH and to target/ for upgrade.sh compatibility)
COPY --from=builder /build/target/release/HIVE /usr/local/bin/hive
RUN mkdir -p target/release && cp /usr/local/bin/hive target/release/HIVE

# Copy configuration files
COPY .env.example .env
COPY README.md .
# Copy persona example as reference (NOT as live persona — onboarding creates that)
COPY persona.toml.example .hive/persona.toml.example

# Copy Flux scripts (MUST be after WORKDIR so they land at /home/hive/src/computer/)
COPY src/computer/generate_image.py src/computer/generate_image.py
COPY src/computer/flux_server.py src/computer/flux_server.py

# Copy source code so codebase_read can inspect the engine
COPY src/ src/

# Copy build manifest for self-recompilation support
COPY Cargo.toml Cargo.lock ./
COPY upgrade.sh ./

# Copy cached dependency artifacts from builder (so cargo build only recompiles HIVE src)
COPY --from=builder --chown=hive:hive /build/target/release/deps target/release/deps
COPY --from=builder --chown=hive:hive /build/target/release/build target/release/build
COPY --from=builder --chown=hive:hive /usr/local/cargo/registry /usr/local/cargo/registry

# Create required directories and copy training scripts to working dir
RUN mkdir -p memory .hive training logs && \
    chown -R hive:hive /home/hive
COPY training/*.py training/

# Final ownership pass — covers cargo/rustup toolchain + all working dirs
RUN chown -R hive:hive /home/hive/training /home/hive/target \
    /usr/local/cargo /usr/local/rustup

# ── Entrypoint script ──────────────────────────────────────────────
COPY <<'ENTRYPOINT' /usr/local/bin/start-hive.sh
#!/bin/bash
set -e

# Establish Internal Git Identity & Bypasses for Host Binding
git config --global user.name "Apis (HIVE)"
git config --global user.email "apis@hive.engine"
git config --global --add safe.directory /home/hive

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
export HIVE_PROJECT_DIR=/home/hive

# Point at host Ollama
export OLLAMA_BASE_URL="${OLLAMA_URL}"

# Point at host Flux server (GPU on host, HTTP from container)
# Fallback: if no host server, use container's Python directly (CPU — slow but works)
export HIVE_FLUX_URL="http://host.docker.internal:8490"
export HIVE_PYTHON_BIN="python3"

# Run HIVE in a restart loop for self-recompilation support.
# Exit code 42 = "restart me with new binary" (self-recompile)
# Any other exit code = normal shutdown (container stops)
while true; do
    /usr/local/bin/hive
    EXIT_CODE=$?

    if [ "$EXIT_CODE" -eq 42 ]; then
        echo ""
        echo "🔄 ═══════════════════════════════════════════════════════"
        echo "🔄  SELF-RECOMPILE: Swapping binary and restarting..."
        echo "🔄 ═══════════════════════════════════════════════════════"

        # Swap the compiled binary into place
        if [ -f /home/hive/HIVE_next ]; then
            cp /home/hive/HIVE_next /usr/local/bin/hive
            cp /home/hive/HIVE_next /home/hive/target/release/HIVE
            rm /home/hive/HIVE_next
            echo "🔄 Binary swapped successfully."
        else
            echo "⚠️  HIVE_next not found — restarting with current binary."
        fi

        # Brief pause to let ports fully release
        sleep 3
        echo "🔄 Restarting HIVE..."
        echo ""
    else
        echo "🐝 HIVE exited with code $EXIT_CODE. Container stopping."
        exit $EXIT_CODE
    fi
done
ENTRYPOINT
RUN chmod +x /usr/local/bin/start-hive.sh

# Expose all ports
EXPOSE 3030 3031 3032 3033 3034 3035 3037 3038 8421 8422 8480

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:3035/api/status || exit 1

USER hive
ENTRYPOINT ["/usr/local/bin/start-hive.sh"]
