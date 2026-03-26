#!/usr/bin/env bash

# Set the working directory to where this script is located
cd "$(dirname "$0")"

echo "========================================"
echo "          Starting HIVE Engine          "
echo "========================================"

# Load Discord token from .env file if it exists
if [ -f .env ]; then
    set -a
    source .env
    set +a
fi

# Verify the token is set
if [ -z "$DISCORD_TOKEN" ]; then
    echo "ERROR: DISCORD_TOKEN is not set."
    echo "Create a .env file with: DISCORD_TOKEN=\"your_token_here\""
    exit 1
fi

# Always enforce correct Ollama parallelism settings.
# OLLAMA_NUM_PARALLEL must be set BEFORE 'ollama serve' starts —
# exporting it after Ollama is already running has no effect.
export OLLAMA_NUM_PARALLEL=16
export OLLAMA_MAX_QUEUE=32
export HIVE_MAX_PARALLEL=16

# macOS Ollama.app doesn't inherit shell env vars — inject via launchctl
if [ "$(uname)" = "Darwin" ]; then
    launchctl setenv OLLAMA_NUM_PARALLEL 16
    launchctl setenv OLLAMA_MAX_QUEUE 32
fi

if pgrep -f "/Applications/Ollama.app" > /dev/null 2>&1; then
    # Ollama is running as macOS app — restart it to pick up launchctl env
    echo "Ollama.app detected — restarting to enforce OLLAMA_NUM_PARALLEL=16..."
    pkill -f "/Applications/Ollama.app" 2>/dev/null || true
    sleep 2
    open -a Ollama
    sleep 4
elif curl -s http://localhost:11434/api/tags > /dev/null 2>&1; then
    # Ollama running as CLI — restart with correct env
    echo "Ollama CLI detected — restarting to enforce OLLAMA_NUM_PARALLEL=16..."
    pkill -f "ollama serve" 2>/dev/null || true
    sleep 1
    ollama serve &
    sleep 3
else
    # Not running at all — start fresh
    echo "Starting Ollama with NUM_PARALLEL=16..."
    ollama serve &
    sleep 3
fi

# Verify Ollama is responsive
if curl -s http://localhost:11434/api/tags > /dev/null 2>&1; then
    echo "✅ Ollama running with OLLAMA_NUM_PARALLEL=16"
else
    echo "⚠️  Ollama failed to start — check logs"
fi

# Build and run the HIVE application
echo "Booting Apis..."
cargo run --release
