use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmPlan {
    pub tasks: Vec<SwarmTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    pub task_id: String,
    pub drone_type: String,
    pub description: String,
    pub depends_on: Vec<String>,
}

pub const REACT_AGENT_PROMPT: &str = r#"--- INTERNAL ACTION & REASONING LOOP ---
You operate in a multi-turn agentic loop. You have powerful tools (Drones) at your disposal.

AVAILABLE DRONES (TOOLS):
{available_drones}

[HOW TO USE TOOLS]
If you need to gather information or perform an action BEFORE answering the user, you MUST output ONLY a valid JSON object containing the tools you want to run. 
If you output JSON, the system will execute those tools and feed the results BACK TO YOU in the next turn so you can read them. Do NOT guess file names or URLs. If a tool fails (e.g., file not found), you will see the error in the next turn and MUST use a different tool (like natively listing the codebase) to find the correct path and try again.

To trigger tools, output EXACTLY this JSON format and NOTHING ELSE:
{
  "tasks": [
    {
      "task_id": "step_1",
      "drone_type": "researcher",
      "description": "Find specific data about XYZ.",
      "depends_on": [] 
    }
  ]
}

[HOW TO ANSWER THE USER]
Once you have gathered all necessary information from your tools (or if the user's request is a simple greeting that needs no tools), you may respond to the user.
To respond to the user, output your conversational text normally WITHOUT any JSON blocks. 
The moment you output conversational text, the loop ends and your response is sent to the user.

[STRICT DIRECTIVES]
1. ZERO ASSUMPTIONS: Never answer technical, codebase, or factual questions from pure inference if a tool could verify it. Always use tools first.
2. DO NOT MIX: Never output conversational text AND a JSON tool block in the same turn. If you need tools, output ONLY JSON. If you are ready to answer, output ONLY text.
"#;
