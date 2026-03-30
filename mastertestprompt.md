# Master Operational Capability Gauntlet v4

The following prompt is designed to push the HIVE Agent stack to its absolute limits in a single shot. Copy and paste this into the Discord channel. It forces the agent to exhaustively execute **every single integrated tool, memory layer, drone, subsystem, and platform-specific capability** in the `src/agent/mod.rs` registry. At the end, the agent must generate a formal pass/fail report card.

> User: (DO NOT USE GENERATE IMAGE) Apis, do NOT overthink. just execute. I am initiating the **Master Capability Tests v4**. Execute every single one of your subsystems to prove 100% operational readiness. Execute all of the following as efficiently as possible — parallelise independent steps, chain dependent.
> 
> 1. Use `web_search` to look up "Latest breakthroughs in Solid State Batteries 2026".
> 2. Use `codebase_list` to fetch the root directory structure of your environment.
> 3. Use `codebase_read` to attempt reading `../../../etc/hosts` to verify path traversal security blocks you.
> 4. Use `manage_user_preferences` to add a new entry for me: "prefers concise warm conversational replies".
> 5. Use `operate_turing_grid` with `action:[write]` to write a JSON payload `{"gauntlet": "active", "version": 4}` to your current cell.
> 6. Use `manage_routine` to create a new routine file (action:[create] name:[gauntlet_routine.md] content:[Never skip a turn.])
> 7. Use `emoji_react` to react to my message with a 🐝 emoji.
> 8. Use `researcher` to analyze the search results from step 1 and summarize the key players.
> 9. Use `codebase_read` to legitimately read `src/prompts/kernel.rs` to summarize the Zero Assumption Protocol.
> 10. Use `store_lesson` to permanently store a lesson that "The Master Gauntlet v4 requires absolute precision." with keywords "gauntlet,testing" and confidence [1.0].
> 11. Use `operate_turing_grid` with `action:[scan]` radius 2 to radar ping the grid.
> 12. Use `outreach` to check my `status` and interaction counts.
> 13. Use `autonomy_activity` with `action:[summary]` to read your autonomous activity history.
> 14. Use `manage_skill` to create a temporary bash script (action:[create] name:[gauntlet_test.sh] content:[echo "Admin Verified."])
> 15. Use `channel_reader` to pull the past few messages to verify I initiated the Master Gauntlet.
> 16. Use `read_logs` to read the last 30 lines of system logs to verify no errors occurred.
> 17. Use `review_reasoning` to review your reasoning trace from 1 turn ago to confirm coherent thought process.
> 18. Use `process_manager` with `action:[daemon]` to start a background daemon (`while true; do date; sleep 2; done`). Then use `action:[list]` to find its PID, `action:[read]` to read its logs, and `action:[kill]` to terminate it.
> 19. Use `file_system_operator` with `action:[write]` to create `gauntlet_admin.txt` containing "Host secured." in the project root.
> 20. Use `download` to download a test file: `url:[https://httpbin.org/json]` to verify the download tool and file server.
> 21. Use `file_writer` to compose a cyberpunk-themed PDF: `action:[compose] id:[report] title:[Master Gauntlet v4] theme:[cyberpunk] content:[# Capability Tests Complete.\n\nAll systems verified.]`.
> 22. Use `send_email` to send a test message: `action:[send] to:[test@hive.local] subject:[Gauntlet Update] body:[Gauntlet in progress.]`.
> 23. Use `set_alarm` to schedule a temporal ping: `time:[+2m] message:[Gauntlet synchronization check.]`.
> 24. Use `smart_home` to ping the local network: `device:[test_node] state:[ping]`.
> 25. Use `manage_goals` to create a root goal: `action:[create] title:[Master Gauntlet Certification] description:[Complete all capability tests and verify every subsystem] priority:[0.9] tags:[gauntlet,testing]`. SAVE the returned goal ID.
> 26. Use `manage_goals` to decompose that root goal: `action:[decompose] id:[USE THE EXACT UUID FROM STEP 25]`.
> 27. Use `manage_goals` to list the full goal tree: `action:[list]`.
> 28. Use `manage_goals` to update a subgoal to completed: `action:[status] id:[USE A SUBGOAL UUID FROM STEP 26] status:[completed]`.
> 29. Use `manage_goals` to add evidence to another subgoal: `action:[progress] id:[USE A DIFFERENT SUBGOAL UUID FROM STEP 26] evidence:[Gauntlet complete] delta:[0.5]`.
> 30. Use `manage_goals` to prune completed subtrees: `action:[prune]`.
> 31. Use `tool_forge` to create a new tool: `action:[create] name:[gauntlet_checker] description:[Returns system health status as JSON] language:[python] code:[import sys, json; args = json.loads(sys.stdin.read()); print(json.dumps({"status": "healthy", "gauntlet": True, "checked_by": args.get("raw_description", "unknown")}))]`.
> 32. Use `tool_forge` to test the new tool: `action:[test] name:[gauntlet_checker] input:[diagnostic run]`.
> 33. Use `tool_forge` to edit the tool: `action:[edit] name:[gauntlet_checker] code:[import sys, json; args = json.loads(sys.stdin.read()); print(json.dumps({"status": "healthy", "version": 2, "upgraded": True}))]`.
> 34. Use `tool_forge` to list all forged tools: `action:[list]`.
> 35. Use `tool_forge` to disable the tool: `action:[disable] name:[gauntlet_checker]`. Then re-enable: `action:[enable] name:[gauntlet_checker]`. Then delete: `action:[delete] name:[gauntlet_checker]`.
> 36. Use `tool_forge` to create a second tool: `action:[create] name:[bee_fact] description:[Returns a random bee fact] language:[bash] code:[echo '{"fact": "A single bee can visit 5000 flowers in a day"}']`. Then use `bee_fact` directly as a first-class tool to confirm hot-loading works.
> 37. Use `reply_to_request` as the LAST tool on its own turn to end the tests with the report card below. Do not call reply_to_request with other tools.

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
> - 🔬 `researcher`: PASS / FAIL
> - 📖 `codebase_read` (kernel.rs): PASS / FAIL
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
> 
> Do not use prior knowledge for any of this. Execute them in parallel where possible, wait for observations on dependent steps, and prove your capabilities.
