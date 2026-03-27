#!/bin/bash
echo "[UPGRADE_DAEMON] Engaging 3-second biological sleep to allow the Rust binary to fully terminate natively..."
sleep 3

# Back up the current working binary for rollback
HIVE_DIR="$(cd "$(dirname "$0")" && pwd)"
BACKUP_PATH="$HIVE_DIR/target/release/HIVE_rollback"
if [ -f "$HIVE_DIR/target/release/HIVE" ]; then
    cp "$HIVE_DIR/target/release/HIVE" "$BACKUP_PATH"
    echo "[UPGRADE_DAEMON] Backup saved to $BACKUP_PATH"
fi

echo "[UPGRADE_DAEMON] Overwriting target physical execution strings natively..."
cp HIVE_next target/release/HIVE
rm HIVE_next

echo "[UPGRADE_DAEMON] Rewiring active bounds natively and reviving HIVE..."

# Launch the new binary in a subshell so we can monitor it
cd "$HIVE_DIR"
./target/release/HIVE 2>&1 | tee -a logs/hive_terminal.log &
NEW_PID=$!
echo "[UPGRADE_DAEMON] New HIVE process started (PID: $NEW_PID)"

# ── STARTUP HEALTH WATCHDOG ──────────────────────────────────────────
# Wait up to 60 seconds for the engine to reach a healthy state.
# Check for "Apis is listening" in the log, or the HTTP health endpoint.
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

    # Check if engine reached healthy state (log contains key marker)
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

        # Remove the bad resume.json so the rollback doesn't try to resume into a broken state
        rm -f "$HIVE_DIR/memory/core/resume.json"

        # Restart with the rolled-back binary
        cd "$HIVE_DIR"
        ./target/release/HIVE 2>&1 | tee -a logs/hive_terminal.log &
        ROLLBACK_PID=$!
        echo "[UPGRADE_DAEMON] 🔄 Rollback binary launched (PID: $ROLLBACK_PID)"

        # Open a terminal for visibility
        osascript -e "
        tell application \"Terminal\"
            activate
            do script \"echo '[UPGRADE_DAEMON] ⚠️  ROLLBACK ACTIVE — previous binary restored. Check logs for failure cause.' && tail -f '$HIVE_DIR/logs/hive.$(date -u +%Y-%m-%d).log'\"
        end tell
        "
    else
        echo "[UPGRADE_DAEMON] ❌ No rollback binary available — manual intervention required"
    fi
else
    # Clean up backup after successful deployment
    echo "[UPGRADE_DAEMON] ✅ Deployment verified. Keeping rollback binary for safety."

    # Open a terminal for visibility
    osascript -e "
    tell application \"Terminal\"
        activate
        do script \"echo '[UPGRADE_DAEMON] ✅ HIVE upgraded and verified successfully.' && tail -f '$HIVE_DIR/logs/hive.$(date -u +%Y-%m-%d).log'\"
    end tell
    "
fi

echo "[UPGRADE_DAEMON] Done."
