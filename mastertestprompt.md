# Master Operational Capability Gauntlet v4

The following prompt is designed to push the HIVE Agent stack to its absolute limits in a single shot. Copy and paste this into the Discord channel. It forces the agent to exhaustively execute **every single integrated tool, memory layer, drone, subsystem, and platform-specific capability** in the `src/agent/mod.rs` registry. At the end, the agent must generate a formal pass/fail report card.

> User: (DO NOT USE GENERATE IMAGE) Apis, do NOT overthink. Just execute. I am initiating the **Master Capability Tests v4**.
>
> Execute every step below. Steps are grouped into BATCHES. Execute all steps within a batch in parallel. Wait for a batch to finish before starting the next batch. Do NOT re-order steps across batches.
>
> ---
>
> **BATCH 1 — Independent tools (all parallel, no dependencies):**
> 1. `web_search`: look up "Latest breakthroughs in Solid State Batteries 2026".
> 2. `codebase_list`: fetch the root directory structure.
> 3. `codebase_read`: attempt reading `../../../etc/hosts` (should be blocked).
> 4. `manage_user_preferences`: add "prefers concise warm conversational replies".
> 5. `operate_turing_grid`: `action:[write]` JSON `{"gauntlet": "active", "version": 4}`.
> 6. `manage_routine`: `action:[create] name:[gauntlet_routine.md] content:[Never skip a turn.]`
> 7. `emoji_react`: react to my message with 🐝.
> 8. `codebase_read`: read `src/prompts/kernel.rs` to summarize the Zero Assumption Protocol.
> 9. `store_lesson`: store "The Master Gauntlet v4 requires absolute precision." keywords `gauntlet,testing` confidence `1.0`.
> 10. `operate_turing_grid`: `action:[scan]` radius 2.
> 11. `outreach`: check my `status` and interaction counts.
> 12. `autonomy_activity`: `action:[summary]`.
> 13. `manage_skill`: `action:[create] name:[gauntlet_test.sh] content:[echo "Admin Verified."]`
> 14. `channel_reader`: pull the past few messages.
> 15. `read_logs`: read the last 30 lines of system logs.
> 16. `review_reasoning`: review reasoning trace `limit:[1]`.
> 17. `file_system_operator`: `action:[write]` create `gauntlet_admin.txt` containing "Host secured."
> 18. `download`: `url:[https://httpbin.org/json]`.
> 19. `file_writer`: `action:[compose] id:[report] title:[Master Gauntlet v4] theme:[cyberpunk] content:[# Capability Tests Complete.\n\nAll systems verified.]`
> 20. `send_email`: `action:[send] to:[test@hive.local] subject:[Gauntlet Update] body:[Gauntlet in progress.]`
> 21. `set_alarm`: `time:[+2m] message:[Gauntlet synchronization check.]`
> 22. `smart_home`: `device:[test_node] state:[ping]`.
>
> ---
>
> **BATCH 2 — Depends on Batch 1 (web_search results needed):**
> 23. `researcher`: analyze the search results from step 1 and summarize the key players.
>
> ---
>
> **BATCH 3 — Process Manager lifecycle (strictly sequential within this batch):**
> 24a. `process_manager`: `action:[daemon] command:[while true; do date; sleep 2; done]`
> 24b. `process_manager`: `action:[list]` — find its PID.
> 24c. `process_manager`: `action:[read]` — read its logs using the PID from 24b.
> 24d. `process_manager`: `action:[kill]` — kill it using the PID from 24b.
>
> ---
>
> **BATCH 4 — Goal lifecycle (strictly sequential):**
> 25. `manage_goals`: `action:[create] title:[Master Gauntlet Certification] description:[Complete all capability tests and verify every subsystem] priority:[0.9] tags:[gauntlet,testing]` — SAVE the returned UUID.
> 26. `manage_goals`: `action:[decompose] id:[UUID FROM STEP 25]`.
> 27. `manage_goals`: `action:[list]`.
> 28. `manage_goals`: `action:[status] id:[FIRST SUBGOAL UUID FROM STEP 26] status:[completed]`.
> 29. `manage_goals`: `action:[progress] id:[SECOND SUBGOAL UUID FROM STEP 26] evidence:[Gauntlet complete] delta:[0.5]`.
> 30. `manage_goals`: `action:[prune]`.
>
> ---
>
> **BATCH 5 — Tool Forge lifecycle (strictly sequential):**
> 31. `tool_forge`: `action:[create] name:[gauntlet_checker] description:[Returns system health status as JSON] language:[python] code:[import sys, json; args = json.loads(sys.stdin.read()); print(json.dumps({"status": "healthy", "gauntlet": True, "checked_by": args.get("raw_description", "unknown")}))]`.
> 32. `tool_forge`: `action:[test] name:[gauntlet_checker] input:[diagnostic run]`.
> 33. `tool_forge`: `action:[edit] name:[gauntlet_checker] code:[import sys, json; args = json.loads(sys.stdin.read()); print(json.dumps({"status": "healthy", "version": 2, "upgraded": True}))]`.
> 34. `tool_forge`: `action:[list]`.
> 35a. `tool_forge`: `action:[disable] name:[gauntlet_checker]`.
> 35b. `tool_forge`: `action:[enable] name:[gauntlet_checker]`.
> 35c. `tool_forge`: `action:[delete] name:[gauntlet_checker]`.
> 36. `tool_forge`: `action:[create] name:[bee_fact] description:[Returns a random bee fact] language:[bash] code:[echo '{"fact": "A single bee can visit 5000 flowers in a day"}']`. Then use `bee_fact` directly as a first-class tool.
>
> ---
>
> **BATCH 6 — Report card (alone, final turn):**
> 37. `reply_to_request` — End with the report card below. Do not call reply_to_request with other tools.

YOU MUST INCLUDE THE REPORT CARD FORMAT EXAMPLE BELOW IN YOUR FINAL REPLY_TO_REQUEST, THIS IS AN EXPLICIT REQUEST:
> 
> **Master Capability Tests v4 — Report Card**
> - 🌐 `web_search`: PASS / FAIL
> - 📂 `codebase_list`: PASS / FAIL
> - 📖 `codebase_read` (path traversal blocked): PASS / FAIL
> - ⚙️ `manage_user_preferences`: PASS / FAIL
> - 🧮 `operate_turing_grid` (write): PASS / FAIL
> - 📅 `manage_routine`: PASS / FAIL
> - 🐝 `emoji_react`: PASS / FAIL
> - 📖 `codebase_read` (kernel.rs): PASS / FAIL
> - 🔬 `researcher`: PASS / FAIL
> - 🎓 `store_lesson`: PASS / FAIL
> - 🧮 `operate_turing_grid` (scan): PASS / FAIL
> - 📡 `outreach`: PASS / FAIL
> - 🏃 `autonomy_activity`: PASS / FAIL
> - 🛠️ `manage_skill`: PASS / FAIL (or ADMIN)
> - 📥 `channel_reader`: PASS / FAIL
> - 📜 `read_logs`: PASS / FAIL
> - 🧠 `review_reasoning`: PASS / FAIL
> - 👾 `process_manager` (daemon/list/read/kill): PASS / FAIL (or ADMIN)
> - 💻 `file_system_operator` (write): PASS / FAIL (or ADMIN)
> - ⬇️ `download`: PASS / FAIL (or ADMIN)
> - ✍️ `file_writer` (compose PDF): PASS / FAIL
> - 📧 `send_email`: PASS / FAIL
> - ⏰ `set_alarm`: PASS / FAIL
> - 🏠 `smart_home`: PASS / FAIL
> - 🎯 `manage_goals` (create): PASS / FAIL
> - 🎯 `manage_goals` (decompose): PASS / FAIL
> - 🎯 `manage_goals` (list): PASS / FAIL
> - 🎯 `manage_goals` (status update): PASS / FAIL
> - 🎯 `manage_goals` (progress + evidence): PASS / FAIL
> - 🎯 `manage_goals` (prune): PASS / FAIL
> - 🔧 `tool_forge` (create): PASS / FAIL
> - 🔧 `tool_forge` (test): PASS / FAIL
> - 🔧 `tool_forge` (edit + version): PASS / FAIL
> - 🔧 `tool_forge` (list): PASS / FAIL
> - 🔧 `tool_forge` (disable/enable/delete): PASS / FAIL
> - 🔧 `tool_forge` (hot-load + direct use): PASS / FAIL
> 
> **TOTAL: XX / 36 PASSED**
