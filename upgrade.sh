#!/bin/bash
echo "[UPGRADE_DAEMON] Engaging upgrade sequence..."

# Detect Docker
IS_DOCKER=false
if [ -f /.dockerenv ] || grep -q docker /proc/1/cgroup 2>/dev/null; then
    IS_DOCKER=true
fi

# Back up the current working binary for rollback
HIVE_DIR="$(cd "$(dirname "$0")" && pwd)"
BACKUP_PATH="$HIVE_DIR/target/release/HIVE_rollback"
if [ -f "$HIVE_DIR/target/release/HIVE" ]; then
    cp "$HIVE_DIR/target/release/HIVE" "$BACKUP_PATH"
    echo "[UPGRADE_DAEMON] Backup saved to $BACKUP_PATH"
fi

echo "[UPGRADE_DAEMON] Overwriting binary..."
cp HIVE_next target/release/HIVE
rm HIVE_next

# In Docker, also update the PATH binary
if [ "$IS_DOCKER" = true ] && [ -w /usr/local/bin/hive ]; then
    cp target/release/HIVE /usr/local/bin/hive
    echo "[UPGRADE_DAEMON] Updated /usr/local/bin/hive for Docker"
fi

# ── KILL OLD PROCESS AND WAIT FOR PORTS ──────────────────────────────
# The old HIVE process has servers bound to ports 3030-3038, 8421, 8480.
# We must wait for those ports to be released before starting the new binary.
PORTS="3030 3031 3032 3033 3034 3035 3037 3038 8421 8480"

# Kill any remaining HIVE processes (except this script and its parent)
echo "[UPGRADE_DAEMON] Killing old HIVE processes..."
for pid in $(pgrep -f "target/release/HIVE" 2>/dev/null || true); do
    if [ "$pid" != "$$" ] && [ "$pid" != "$PPID" ]; then
        kill "$pid" 2>/dev/null
    fi
done

# Also kill by the PATH binary name
for pid in $(pgrep -f "/usr/local/bin/hive" 2>/dev/null || true); do
    if [ "$pid" != "$$" ] && [ "$pid" != "$PPID" ]; then
        kill "$pid" 2>/dev/null
    fi
done

# Wait for ports to actually free (max 30 seconds)
echo "[UPGRADE_DAEMON] Waiting for ports to release..."
TIMEOUT=30
ELAPSED=0
while [ $ELAPSED -lt $TIMEOUT ]; do
    BUSY=false
    for port in $PORTS; do
        if lsof -i ":$port" -t >/dev/null 2>&1; then
            BUSY=true
            break
        fi
    done

    if [ "$BUSY" = false ]; then
        echo "[UPGRADE_DAEMON] ✅ All ports released in ${ELAPSED}s"
        break
    fi

    sleep 2
    ELAPSED=$((ELAPSED + 2))
    echo "[UPGRADE_DAEMON] Ports still busy... (${ELAPSED}s/${TIMEOUT}s)"

    # Force kill after 10 seconds
    if [ $ELAPSED -ge 10 ]; then
        for port in $PORTS; do
            lsof -i ":$port" -t 2>/dev/null | xargs kill -9 2>/dev/null
        done
    fi
done

if [ "$BUSY" = true ]; then
    echo "[UPGRADE_DAEMON] ⚠️ Some ports still busy after ${TIMEOUT}s — proceeding anyway"
fi

# ── LAUNCH NEW BINARY ────────────────────────────────────────────────
echo "[UPGRADE_DAEMON] Starting new HIVE binary..."
cd "$HIVE_DIR"
./target/release/HIVE 2>&1 | tee -a logs/hive_terminal.log &
NEW_PID=$!
echo "[UPGRADE_DAEMON] New HIVE process started (PID: $NEW_PID)"

# ── STARTUP HEALTH WATCHDOG ──────────────────────────────────────────
TIMEOUT=60
ELAPSED=0
HEALTHY=false

while [ $ELAPSED -lt $TIMEOUT ]; do
    sleep 5
    ELAPSED=$((ELAPSED + 5))

    # Check if process is still alive
    if ! kill -0 $NEW_PID 2>/dev/null; then
        echo "[UPGRADE_DAEMON] ❌ New HIVE process died (PID: $NEW_PID)"
        break
    fi

    # Check if engine reached healthy state
    if tail -n 50 logs/hive.$(date -u +%Y-%m-%d).log 2>/dev/null | grep -q "Apis is listening"; then
        HEALTHY=true
        echo "[UPGRADE_DAEMON] ✅ HIVE reached healthy state in ${ELAPSED}s"
        break
    fi

    echo "[UPGRADE_DAEMON] Waiting for health signal... (${ELAPSED}s/${TIMEOUT}s)"
done

if [ "$HEALTHY" = false ]; then
    echo "[UPGRADE_DAEMON] ⚠️  HIVE did not reach healthy state within ${TIMEOUT}s"

    # Kill the stuck process
    kill $NEW_PID 2>/dev/null
    sleep 2
    kill -9 $NEW_PID 2>/dev/null

    # Rollback to previous binary
    if [ -f "$BACKUP_PATH" ]; then
        echo "[UPGRADE_DAEMON] 🔄 ROLLING BACK to previous binary..."
        cp "$BACKUP_PATH" "$HIVE_DIR/target/release/HIVE"

        if [ "$IS_DOCKER" = true ] && [ -w /usr/local/bin/hive ]; then
            cp "$BACKUP_PATH" /usr/local/bin/hive
        fi

        rm -f "$HIVE_DIR/memory/core/resume.json"

        # Wait for ports again
        sleep 5
        for port in $PORTS; do
            lsof -i ":$port" -t 2>/dev/null | xargs kill -9 2>/dev/null
        done
        sleep 2

        cd "$HIVE_DIR"
        ./target/release/HIVE 2>&1 | tee -a logs/hive_terminal.log &
        ROLLBACK_PID=$!
        echo "[UPGRADE_DAEMON] 🔄 Rollback binary launched (PID: $ROLLBACK_PID)"

        # Open a terminal for visibility (macOS only — skip in Docker)
        if [ "$IS_DOCKER" = false ] && command -v osascript &>/dev/null; then
            osascript -e "
            tell application \"Terminal\"
                activate
                do script \"echo '[UPGRADE_DAEMON] ⚠️  ROLLBACK ACTIVE — previous binary restored. Check logs for failure cause.' && tail -f '$HIVE_DIR/logs/hive.$(date -u +%Y-%m-%d).log'\"
            end tell
            "
        fi
    else
        echo "[UPGRADE_DAEMON] ❌ No rollback binary available — manual intervention required"
    fi
else
    echo "[UPGRADE_DAEMON] ✅ Deployment verified. Keeping rollback binary for safety."

    # Open a terminal for visibility (macOS only — skip in Docker)
    if [ "$IS_DOCKER" = false ] && command -v osascript &>/dev/null; then
        osascript -e "
        tell application \"Terminal\"
            activate
            do script \"echo '[UPGRADE_DAEMON] ✅ HIVE upgraded and verified successfully.' && tail -f '$HIVE_DIR/logs/hive.$(date -u +%Y-%m-%d).log'\"
        end tell
        "
    fi
fi

echo "[UPGRADE_DAEMON] Done."
