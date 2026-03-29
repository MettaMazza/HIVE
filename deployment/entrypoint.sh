#!/usr/bin/env bash
set -euo pipefail

# ─── Config ───────────────────────────────────────────────────────────────────
MODEL="${HIVE_MODEL:-qwen3.5:9b}"
OLLAMA_HOST="http://localhost:11434"
MAX_WAIT=300   # seconds to wait for Ollama to be ready
MODEL_DIR="${OLLAMA_MODELS:-/ollama-models}"

echo "=== HIVE Container Startup ==="
echo "  Model : $MODEL"
echo "  Models: $MODEL_DIR"

# ─── Parallelism — must be set before ollama serve starts ────────────────────
# Mirrors the logic in start_hive.sh: all three vars set together.
export OLLAMA_NUM_PARALLEL="${OLLAMA_NUM_PARALLEL:-1}"
export OLLAMA_MAX_QUEUE="${OLLAMA_MAX_QUEUE:-32}"
export OLLAMA_KV_CACHE_TYPE="${OLLAMA_KV_CACHE_TYPE:-q4_0}"
export HIVE_MAX_PARALLEL="${HIVE_MAX_PARALLEL:-2}"

# ─── Start Ollama in background ───────────────────────────────────────────────
OLLAMA_MODELS="$MODEL_DIR" ollama serve &
OLLAMA_PID=$!

# ─── Wait for Ollama to be responsive ────────────────────────────────────────
echo "Waiting for Ollama..."
elapsed=0
until curl -sf "$OLLAMA_HOST/api/tags" > /dev/null 2>&1; do
    sleep 2
    elapsed=$((elapsed + 2))
    if [ $elapsed -ge $MAX_WAIT ]; then
        echo "ERROR: Ollama did not start within ${MAX_WAIT}s" >&2
        exit 1
    fi
done
echo "Ollama ready after ${elapsed}s"

# ─── Pull model if not already cached ─────────────────────────────────────────
if ! ollama list | grep -q "^${MODEL}"; then
    echo "Pulling model: $MODEL (this may take a while on first run)"
    ollama pull "$MODEL"
else
    echo "Model $MODEL already cached — skipping pull"
fi

# ─── Warm up model (load runner into GPU memory) ──────────────────────────────
echo "Warming up model..."
warmup_start=$(date +%s)
ollama run "$MODEL" "hi" --verbose 2>/dev/null | head -1 || true
warmup_elapsed=$(($(date +%s) - warmup_start))
echo "Model warm after ${warmup_elapsed}s"

# ─── Ensure log dir is writable by hive user ──────────────────────────────────
# Named volumes are owned by root. HIVE runs as the hive user, so fix perms here.
mkdir -p /app/logs /app/data /app/memory/core
chown -R hive:hive /app/logs /app/data /app/memory/core

echo "Booting Apis..."
cd /app
exec gosu hive /app/hive

# Cleanup (exec replaces shell so this only runs on abnormal exit)
kill "$OLLAMA_PID" 2>/dev/null || true
