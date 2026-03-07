#!/usr/bin/env bash

# Set the working directory to where this script is located
cd "$(dirname "$0")"

echo "========================================"
echo "          Starting HIVE Engine          "
echo "========================================"

# Load Discord token from .env file if it exists
if [ -f .env ]; then
    export $(grep -v '^#' .env | xargs)
fi

# Verify the token is set
if [ -z "$DISCORD_TOKEN" ]; then
    echo "ERROR: DISCORD_TOKEN is not set."
    echo "Create a .env file with: DISCORD_TOKEN=\"your_token_here\""
    exit 1
fi

# Check if the Ollama API is responsive
if ! curl -s http://localhost:11434/api/tags > /dev/null; then
    echo "Ollama is not running. Attempting to start 'ollama serve' in the background..."
    ollama serve &
    sleep 3
fi

# Build and run the HIVE application
echo "Booting Apis..."
cargo run --release
