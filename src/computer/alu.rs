use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone)]
pub struct ALU {
    runtime_dir: PathBuf,
}

impl Default for ALU {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ALU {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        let runtime_dir = base_dir.unwrap_or_else(|| PathBuf::from("memory/computer_runtime"));
        Self { runtime_dir }
    }

    pub async fn init(&self) -> std::io::Result<()> {
        if !self.runtime_dir.exists() {
            fs::create_dir_all(&self.runtime_dir).await?;
        }
        Ok(())
    }

    pub async fn execute_cell(&self, format: &str, content: &str) -> Result<String, String> {
        self.init().await.map_err(|e| format!("Init error: {}", e))?;
        
        match format.to_lowercase().as_str() {
            "python" | "py" => self.run_script(content, "python3", "py").await,
            "sh" | "bash" => self.run_script(content, "bash", "sh").await,
            "javascript" | "js" | "node" => self.run_script(content, "node", "js").await,
            "ruby" | "rb" => self.run_script(content, "ruby", "rb").await,
            "swift" => self.run_script(content, "swift", "swift").await,
            "applescript" | "osascript" => self.run_script(content, "osascript", "applescript").await,
            "perl" | "pl" => self.run_script(content, "perl", "pl").await,
            "rust" | "rs" => self.run_rust_script(content).await,
            _ => Err(format!("Unsupported execution format: {}", format)),
        }
    }

    async fn run_rust_script(&self, code: &str) -> Result<String, String> {
        let timestamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let script_id = format!("rust_script_{}.rs", timestamp);
        let binary_id = format!("rust_bin_{}", timestamp);
        
        let script_path = self.runtime_dir.join(&script_id);
        fs::write(&script_path, code).await.map_err(|e| e.to_string())?;

        // 1. Compile
        let compile_child = Command::new("rustc")
            .arg(&script_id)
            .arg("-o")
            .arg(&binary_id)
            .current_dir(&self.runtime_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn rustc: {}", e))?;

        let compile_execution = timeout(Duration::from_secs(10), compile_child.wait_with_output()).await;
        
        match compile_execution {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let _ = fs::remove_file(&script_path).await;
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(format!("Rust Compilation Failed:\n{}", stderr));
                }
            }
            Ok(Err(e)) => {
                let _ = fs::remove_file(&script_path).await;
                return Err(format!("I/O Error waiting for compiler: {}", e));
            }
            Err(_) => {
                let _ = fs::remove_file(&script_path).await;
                return Err("Rust Compilation Timeout: Exceeded 10 seconds.".to_string());
            }
        }

        // 2. Execute
        let run_child = Command::new(format!("./{}", binary_id))
            .current_dir(&self.runtime_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn compiled rust binary: {}", e))?;

        let run_execution = timeout(Duration::from_secs(15), run_child.wait_with_output()).await;

        let result = match run_execution {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if output.status.success() {
                    Ok(stdout.trim().to_string())
                } else {
                    Err(format!("Execution Failed.\nSTDOUT:\n{}\nSTDERR:\n{}", stdout, stderr))
                }
            }
            Ok(Err(e)) => Err(format!("I/O Error waiting for child: {}", e)),
            Err(_) => Err("Execution Timeout: Process exceeded 15.0 seconds and was terminated.".to_string()),
        };

        let _ = fs::remove_file(&script_path).await;
        let _ = fs::remove_file(self.runtime_dir.join(&binary_id)).await;
        
        result
    }

    /// Execute a pipeline of cells sequentially, chaining stdout from one cell
    /// as context to the next. Each cell receives the previous output as a
    /// `PIPELINE_INPUT` environment variable.
    pub async fn execute_pipeline(&self, cells: &[(String, String)]) -> Result<String, String> {
        self.init().await.map_err(|e| format!("Init error: {}", e))?;

        if cells.is_empty() {
            return Err("Pipeline is empty — no cells to execute.".to_string());
        }

        let mut previous_output = String::new();
        let mut all_outputs = Vec::new();

        for (i, (format, content)) in cells.iter().enumerate() {
            // Inject previous output as a comment/header prepended to the code
            let augmented_content = if previous_output.is_empty() {
                content.clone()
            } else {
                // Set PIPELINE_INPUT as env var via a shim wrapper
                match format.to_lowercase().as_str() {
                    "python" | "py" => format!(
                        "import os\nos.environ['PIPELINE_INPUT'] = {}\n{}",
                        serde_json::to_string(&previous_output).unwrap_or_default(),
                        content
                    ),
                    "sh" | "bash" => {
                        // Escape single quotes for safe bash injection
                        let escaped = previous_output.replace('\'', "'\\''");
                        format!("export PIPELINE_INPUT='{}'\n{}", escaped, content)
                    }
                    "javascript" | "js" | "node" => format!(
                        "process.env.PIPELINE_INPUT = {};\n{}",
                        serde_json::to_string(&previous_output).unwrap_or_default(),
                        content
                    ),
                    _ => content.clone(),
                }
            };

            match self.execute_cell(format, &augmented_content).await {
                Ok(stdout) => {
                    all_outputs.push(format!(
                        "--- Cell {} ({}) ---\n{}",
                        i + 1, format, stdout
                    ));
                    previous_output = stdout;
                }
                Err(e) => {
                    all_outputs.push(format!(
                        "--- Cell {} ({}) FAILED ---\n{}",
                        i + 1, format, e
                    ));
                    return Err(format!(
                        "Pipeline halted at cell {} ({}).\n\n{}",
                        i + 1,
                        format,
                        all_outputs.join("\n\n")
                    ));
                }
            }
        }

        Ok(all_outputs.join("\n\n"))
    }

    async fn run_script(&self, code: &str, interpreter: &str, ext: &str) -> Result<String, String> {
        let script_id = format!("script_{}.{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), ext);
        let script_path = self.runtime_dir.join(&script_id);
        
        fs::write(&script_path, code).await.map_err(|e| e.to_string())?;

        let child = Command::new(interpreter)
            .arg(&script_id)
            .current_dir(&self.runtime_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn {}: {}", interpreter, e))?;

        let execution = timeout(Duration::from_secs(15), child.wait_with_output());

        let result = match execution.await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if output.status.success() {
                    Ok(stdout.trim().to_string())
                } else {
                    Err(format!("Execution Failed.\nSTDOUT:\n{}\nSTDERR:\n{}", stdout, stderr))
                }
            }
            Ok(Err(e)) => Err(format!("I/O Error waiting for child: {}", e)),
            Err(_) => Err("Execution Timeout: Process exceeded 15.0 seconds and was terminated.".to_string()),
        };

        let _ = fs::remove_file(&script_path).await;
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_alu_initialization() {
        let dir = env::temp_dir().join("hive_turing_test_alu_init");
        let alu = ALU::new(Some(dir.clone()));
        alu.init().await.unwrap();
        assert!(dir.exists());
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_alu_execute_python_basic() {
        let dir = env::temp_dir().join("hive_turing_test_alu_py");
        let alu = ALU::new(Some(dir.clone()));
        let res = alu.execute_cell("python", "print('hello from python')").await;
        assert_eq!(res.unwrap(), "hello from python");
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_alu_execute_bash_basic() {
        let dir = env::temp_dir().join("hive_turing_test_alu_sh");
        let alu = ALU::new(Some(dir.clone()));
        let res = alu.execute_cell("sh", "echo 'hello from bash'").await;
        assert_eq!(res.unwrap(), "hello from bash");
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_alu_execute_unsupported() {
        let dir = env::temp_dir().join("hive_turing_test_alu_unsup");
        let alu = ALU::new(Some(dir.clone()));
        let res = alu.execute_cell("cplusplus", "cout << 'hello'").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Unsupported execution format: cplusplus"));
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_alu_execute_timeout() {
        let dir = env::temp_dir().join("hive_turing_test_alu_timeout");
        let alu = ALU::new(Some(dir.clone()));
        // Note: Implementation uses 15s timeout
        let res = alu.execute_cell("python", "import time\ntime.sleep(16)").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Execution Timeout"));
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_alu_execute_failure() {
        let dir = env::temp_dir().join("hive_turing_test_alu_fail");
        let alu = ALU::new(Some(dir.clone()));
        let res = alu.execute_cell("python", "import sys\nsys.exit(1)").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Execution Failed"));
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_alu_pipeline() {
        let dir = env::temp_dir().join("hive_turing_test_alu_pipeline");
        let alu = ALU::new(Some(dir.clone()));

        let cells = vec![
            ("python".to_string(), "print('42')".to_string()),
            ("bash".to_string(), "echo \"received: $PIPELINE_INPUT\"".to_string()),
        ];
        let result = alu.execute_pipeline(&cells).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("42"));
        assert!(output.contains("received: 42"));

        // Empty pipeline
        let empty = alu.execute_pipeline(&[]).await;
        assert!(empty.is_err());
        assert!(empty.unwrap_err().contains("empty"));

        let _ = fs::remove_dir_all(&dir).await;
    }
}
