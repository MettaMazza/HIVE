#!/bin/bash
echo "[UPGRADE_DAEMON] Engaging 3-second biological sleep to allow the Rust binary to fully terminate natively..."
sleep 3

echo "[UPGRADE_DAEMON] Overwriting target physical execution strings natively..."
cp HIVE_next target/release/HIVE
rm HIVE_next

echo "[UPGRADE_DAEMON] Rewiring active bounds natively and reviving HIVE..."

# Open a NEW visible Terminal window so the operator has full visibility
HIVE_DIR="$(cd "$(dirname "$0")" && pwd)"
osascript -e "
tell application \"Terminal\"
    activate
    do script \"cd '$HIVE_DIR' && echo '[UPGRADE_DAEMON] ✅ HIVE restarted in visible terminal.' && exec ./target/release/HIVE 2>&1 | tee -a logs/hive_terminal.log\"
end tell
"

echo "[UPGRADE_DAEMON] Done. A new Terminal window has been opened with HIVE running."
