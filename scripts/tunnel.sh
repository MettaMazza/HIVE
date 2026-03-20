#!/bin/bash
# HIVE Tunnel Launcher — exposes the file server via localhost.run
# Captures the public URL and writes it to memory/core/tunnel_url.txt
# so Apis can read it and share on request.

TUNNEL_URL_FILE="memory/core/tunnel_url.txt"
PORT="${HIVE_FILE_SERVER_PORT:-8420}"

echo "[TUNNEL] Starting localhost.run tunnel on port $PORT..."
mkdir -p memory/core

# SSH tunnel — capture output to extract the URL
ssh -o StrictHostKeyChecking=no -o ServerAliveInterval=30 -R 80:localhost:$PORT nokey@localhost.run 2>&1 | while IFS= read -r line; do
    echo "[TUNNEL] $line"
    # The actual tunnel URL appears on the "tunneled with tls termination" line
    # e.g. "c8e7fcf1176f8f.lhr.life tunneled with tls termination, https://c8e7fcf1176f8f.lhr.life"
    if echo "$line" | grep -q "tunneled with tls termination"; then
        URL=$(echo "$line" | grep -oE "https://[a-zA-Z0-9._-]+\.[a-z]+\.[a-z]+" | head -1)
        if [ -n "$URL" ]; then
            echo "$URL" > "$TUNNEL_URL_FILE"
            echo "[TUNNEL] ✅ Public URL captured: $URL"
            echo "[TUNNEL] Written to $TUNNEL_URL_FILE"
        fi
    fi
done
