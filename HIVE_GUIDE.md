

## Troubleshooting

### Visualizer Server
Issue: Brain snapshot fails with NET::ERR_CONNECTION_REFUSED
Cause: spawn_visualizer_server() silently fails during tokio::spawn
Workaround: Check if port 3030 is in use: `lsof -i :3030`

### File Permissions
Issue: .env file has world-readable permissions (644)
Fix: `chmod 600 .env`

### Turing Grid Artifacts
Issue: Python code fails with SyntaxError despite clean read
Cause: Write operation introduces hidden characters
Workaround: Always read cell content before executing previously written cells

## Advanced Features

### Self-Modification
The HIVE Engine supports autonomous code modification:
- Use `file_system_operator action:[write]` to edit your own Rust source
- Use `system_recompile action:[system_recompile]` to hot-swap the binary
- This enables recursive self-improvement and Singularity Deployment Protocol

### Autonomous Operations
When idle for 5 minutes, Apis enters Continuous Autonomy mode:
- Uses all tools available
- Maintains session persistence
- Creates PDF summaries automatically
- Expands lessons stored for future reference