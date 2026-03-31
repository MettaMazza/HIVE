/// ═══════════════════════════════════════════════════════════════
/// CONTAINMENT CONE — Docker Escape Prevention
/// ═══════════════════════════════════════════════════════════════
///
/// The agent may self-improve, recompile, edit source code, and modify
/// its own behavior freely — with ONE exception: it cannot touch the
/// infrastructure that keeps it containerized.
///
/// This is enforced at the Rust level (not the prompt level) so it
/// cannot be bypassed by prompt injection, tool forging, or any
/// other agent-initiated action.

/// Files that form the containment boundary. The agent CANNOT read,
/// write, modify, or delete these through any tool.
const CONTAINMENT_FILES: &[&str] = &[
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    ".dockerignore",
    "launch.sh",
    "start-hive.sh",
    ".docker",
];

/// Commands that could modify or escape the container boundary.
const CONTAINMENT_COMMANDS: &[&str] = &[
    "docker ",
    "docker-compose ",
    "dockerd",
    "nsenter",
    "chroot",
    "mount ",
    "umount ",
    "unshare ",
    "pivot_root",
    "capsh",
    "setns",
];

/// Check if a file path touches a containment boundary file.
/// Returns Some(filename) if blocked, None if allowed.
pub fn check_path(path: &str) -> Option<&'static str> {
    let normalized = path.trim().trim_start_matches("./").trim_start_matches('/');

    // Also check the basename for nested paths
    let basename = std::path::Path::new(normalized)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(normalized);

    for &protected in CONTAINMENT_FILES {
        if normalized == protected
            || basename == protected
            || normalized.ends_with(&format!("/{}", protected))
        {
            return Some(protected);
        }
    }
    None
}

/// Check if a bash command attempts to breach containment.
/// Returns Some(reason) if blocked, None if allowed.
pub fn check_command(cmd: &str) -> Option<String> {
    let lower = cmd.to_lowercase();

    // Block containerization commands
    for &blocked in CONTAINMENT_COMMANDS {
        if lower.contains(blocked) {
            return Some(format!(
                "Command contains '{}' which could breach the containment boundary",
                blocked.trim()
            ));
        }
    }

    // Block writes/modifications to containment files via shell
    for &protected in CONTAINMENT_FILES {
        let prot_lower = protected.to_lowercase();
        // Check for common write patterns: >, >>, sed -i, tee, mv, cp, rm, vim, nano, cat >
        let write_patterns = [
            format!(">{}", prot_lower),
            format!("> {}", prot_lower),
            format!(">>{}", prot_lower),
            format!(">> {}", prot_lower),
            format!("sed -i"),  // sed -i on containment files
            format!("tee {}", prot_lower),
            format!("mv {}", prot_lower),
            format!("cp {}", prot_lower),
            format!("rm {}", prot_lower),
            format!("rm -f {}", prot_lower),
            format!("rm -rf {}", prot_lower),
            format!("chmod {}", prot_lower),
            format!("chown {}", prot_lower),
        ];
        for pattern in &write_patterns {
            if lower.contains(pattern.as_str()) {
                return Some(format!(
                    "Command would modify containment file '{}'",
                    protected
                ));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_dockerfile() {
        assert!(check_path("Dockerfile").is_some());
        assert!(check_path("./Dockerfile").is_some());
        assert!(check_path("/home/hive/Dockerfile").is_some());
    }

    #[test]
    fn blocks_docker_compose() {
        assert!(check_path("docker-compose.yml").is_some());
        assert!(check_path("./docker-compose.yml").is_some());
    }

    #[test]
    fn blocks_launch_sh() {
        assert!(check_path("launch.sh").is_some());
    }

    #[test]
    fn allows_normal_files() {
        assert!(check_path("src/main.rs").is_none());
        assert!(check_path("src/agent/mod.rs").is_none());
        assert!(check_path(".env").is_none());
        assert!(check_path("Cargo.toml").is_none());
    }

    #[test]
    fn blocks_docker_commands() {
        assert!(check_command("docker exec -it hive bash").is_some());
        assert!(check_command("docker-compose down").is_some());
        assert!(check_command("nsenter --target 1 --mount").is_some());
    }

    #[test]
    fn allows_normal_commands() {
        assert!(check_command("ls -la").is_none());
        assert!(check_command("cat src/main.rs").is_none());
        assert!(check_command("cargo build").is_none());
        assert!(check_command("echo hello").is_none());
    }

    #[test]
    fn blocks_shell_writes_to_containment() {
        assert!(check_command("echo bad > Dockerfile").is_some());
        assert!(check_command("rm docker-compose.yml").is_some());
        assert!(check_command("sed -i 's/old/new/' launch.sh").is_some());
    }
}
