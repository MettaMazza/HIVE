#!/bin/bash
echo "[UPGRADE_DAEMON] Engaging 3-second biological sleep to allow the Rust binary to fully terminate natively..."
sleep 3

echo "[UPGRADE_DAEMON] Overwriting target physical execution strings natively..."
cp HIVE_next target/release/HIVE
rm HIVE_next

echo "[UPGRADE_DAEMON] Rewiring active bounds natively and reviving HIVE..."
nohup target/release/HIVE > logs/nohup_hive.log 2>&1 &
echo "[UPGRADE_DAEMON] Done. The Engine has ascended."
