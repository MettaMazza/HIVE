# Global build arg — set by docker-compose.gpu.yml for CUDA images.
# Declare before first FROM so it is in scope for all stage targets.
ARG RUNTIME_BASE=debian:bookworm-slim

# ─── Stage 1: Build HIVE ──────────────────────────────────────────────────────
# edition 2024 requires Rust 1.85+
FROM rust:1.88-bookworm AS builder

WORKDIR /build

# Cache dependency compilation separately from source.
# edition 2024 needs both lib.rs and main.rs stubs to satisfy the manifest;
# a bare main.rs alone causes cargo to error before downloading deps.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && \
    echo 'pub fn main() {}' > src/lib.rs && \
    echo 'fn main() {}' > src/main.rs && \
    cargo build --release 2>/dev/null || true && \
    rm -f src/lib.rs src/main.rs

# Build the real source
COPY src ./src
COPY docs ./docs
COPY models ./models
COPY training ./training
# Clean stub artifacts before real build — prevents stale target/ from masking source changes
RUN cargo clean && cargo build --release


# ─── Stage 2: Runtime ─────────────────────────────────────────────────────────
# RUNTIME_BASE is declared globally at the top of this file.
FROM ${RUNTIME_BASE} AS runtime

# Install Ollama dependencies + curl for healthchecks + CA certs for HTTPS
# + google-chrome-stable for headless_chrome PDF generation
# + gosu for privilege drop inside entrypoint (safer than su)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    procps \
    python3 \
    python3-pip \
    zstd \
    lsof \
    gosu \
    && rm -rf /var/lib/apt/lists/*

# Install chrome via direct deb download + dpkg (avoids apt repo GPG key issues)
# Resolve deps after so apt handles the dependency chain properly.
RUN curl -fsSL -o /tmp/chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb && \
    apt-get update && apt-get install -y /tmp/chrome.deb --no-install-recommends && \
    rm /tmp/chrome.deb && \
    rm -rf /var/lib/apt/lists/*

# Tell headless_chrome where to find chrome (auto-detect fails in containers)
ENV CHROME=/usr/bin/google-chrome-stable

# Install Ollama
RUN curl -fsSL https://ollama.ai/install.sh | sh

# Install cloudflared (for quick tunnel public URL)
RUN curl -fsSL https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 \
    -o /usr/local/bin/cloudflared && \
    chmod +x /usr/local/bin/cloudflared

# Create a non-root user for HIVE
RUN groupadd -r hive && useradd -r -g hive -m -d /app hive

WORKDIR /app

# Copy built binary
COPY --from=builder /build/target/release/HIVE /app/hive

# Copy any runtime assets HIVE needs (prompts, training scripts, etc.)
COPY --from=builder /build/src /app/src
COPY training ./training

# Directories that HIVE writes to — owned by the hive user
RUN mkdir -p /app/data /app/logs \
        /app/memory/core \
        /app/memory/cache/tts \
        /app/memory/cache/vision \
        /app/memory/teacher \
        /app/memory/computer_runtime \
        /app/memory/inbox \
    && chown -R hive:hive /app

# Ollama stores models here — this will be a named volume
ENV OLLAMA_MODELS=/ollama-models
RUN mkdir -p /ollama-models && chown -R hive:hive /ollama-models

# Entrypoint script (see entrypoint.sh)
COPY deployment/entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh && chown hive:hive /app/entrypoint.sh

EXPOSE 11434

HEALTHCHECK --interval=30s --timeout=10s --start-period=120s --retries=3 \
    CMD curl -sf http://localhost:11434/api/tags || exit 1

ENTRYPOINT ["/app/entrypoint.sh"]
