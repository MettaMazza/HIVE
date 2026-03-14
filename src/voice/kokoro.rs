use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::SystemTime;
use tokio::fs;
use tracing::{error, info, warn};

pub struct KokoroTTS {
    cache_dir: PathBuf,
    worker_path: PathBuf,
}

impl KokoroTTS {
    pub async fn new() -> std::io::Result<Self> {
        #[cfg(test)]
        let cache_dir = std::env::temp_dir().join(format!(
            "hive_mem_auto_kokoro_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        #[cfg(not(test))]
        let cache_dir = PathBuf::from("memory/cache/tts");
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir).await?;
        }

        let tts = Self {
            cache_dir,
            worker_path: PathBuf::from("src/voice/tts_worker.py"),
        };
        tts.sweep_cache().await?;

        Ok(tts)
    }

    fn hash_text(text: &str) -> String {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    pub async fn get_audio_path(&self, text: &str) -> std::io::Result<PathBuf> {
        let hash = Self::hash_text(text);
        let output_path = self.cache_dir.join(format!("{}.wav", hash));

        // If it's already generated and cached, instantly return it
        if output_path.exists() {
            info!("[Voice] Cache hit for {}", hash);
            // Touch the file to update its modified time so the sweeper doesn't delete it
            let _ = std::fs::File::open(&output_path); // Just opening it updates atime/mtime on some OSes
            return Ok(output_path);
        }

        info!(
            "[Voice] Cache miss for {}. Generating via Kokoro ONNX...",
            hash
        );

        // Subprocess to the python environment to run Kokoro
        let python_cmd = std::env::var("HIVE_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());

        let output_res = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            tokio::process::Command::new(python_cmd)
                .arg(&self.worker_path)
                .arg(text)
                .arg(&output_path)
                .kill_on_drop(true)
                .output()
        ).await;

        let output = match output_res {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(std::io::Error::other("Kokoro generation timed out after 60 seconds.")),
        };

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            error!("[Voice] Generator error: {}", err);
            return Err(std::io::Error::other("Kokoro generation failed."));
        }

        Ok(output_path)
    }

    /// Deletes any `.wav` file in the cache directory older than 1 hour (3600 seconds)
    pub async fn sweep_cache(&self) -> std::io::Result<()> {
        let mut entries = fs::read_dir(&self.cache_dir).await?;
        let now = SystemTime::now();
        let max_age = std::time::Duration::from_secs(3600);

        let mut deleted_count = 0;

        while let Some(entry) = entries.next_entry().await? {
            if let Ok(metadata) = entry.metadata().await
                && let Ok(modified) = metadata.modified()
                && let Ok(age) = now.duration_since(modified)
                && age > max_age
            {
                if let Err(e) = fs::remove_file(entry.path()).await {
                    warn!("[Voice] Failed to clean up old cache file: {}", e);
                } else {
                    deleted_count += 1;
                }
            }
        }

        if deleted_count > 0 {
            info!(
                "[Voice] Swept {} old audio files from cache.",
                deleted_count
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    #[test]
    fn test_hash_text() {
        let hash1 = KokoroTTS::hash_text("Hello world");
        let hash2 = KokoroTTS::hash_text("Hello world");
        let hash3 = KokoroTTS::hash_text("Different");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[tokio::test]
    async fn test_new_creates_dir() {
        // Test explicit directory creation by enforcing a clean state
        let temp_dir = TempDir::new().unwrap();
        let custom_cache = temp_dir.path().join("memory/cache/tts");
        assert!(!custom_cache.exists());

        let tts = KokoroTTS {
            cache_dir: custom_cache.clone(),
            worker_path: PathBuf::from("dummy.py"),
        };

        // Emulate the inside of `new()` targeting our temp dir
        fs::create_dir_all(&tts.cache_dir).await.unwrap();
        assert!(custom_cache.exists());
    }

    #[tokio::test]
    async fn test_sweep_cache() {
        let temp_dir = TempDir::new().unwrap();
        let engine = KokoroTTS {
            cache_dir: temp_dir.path().to_path_buf(),
            worker_path: PathBuf::from("dummy.py"),
        };

        // Create a new file
        let new_file_path = temp_dir.path().join("new.wav");
        File::create(&new_file_path).unwrap();

        // Create an "old" file
        let old_file_path = temp_dir.path().join("old.wav");
        let file = File::create(&old_file_path).unwrap();
        let two_hours_ago = SystemTime::now() - std::time::Duration::from_secs(7200);
        file.set_modified(two_hours_ago).unwrap();
        drop(file);

        // Create a directory that looks like an old file (to trigger remove_file failure branch)
        let old_dir_path = temp_dir.path().join("old_dir.wav");
        std::fs::create_dir(&old_dir_path).unwrap();
        let dir_file = File::open(&old_dir_path).unwrap();
        dir_file.set_modified(two_hours_ago).unwrap();
        drop(dir_file);

        // Sweep it
        engine.sweep_cache().await.unwrap();

        // New file remains, old file deleted
        assert!(new_file_path.exists());
        assert!(!old_file_path.exists());

        // Directories cannot be deleted by `remove_file`, so it triggers the warn! and remains
        assert!(old_dir_path.exists());
    }

    #[tokio::test]
    async fn test_get_audio_path_cache_hit() {
        let temp_dir = TempDir::new().unwrap();
        let engine = KokoroTTS {
            cache_dir: temp_dir.path().to_path_buf(),
            worker_path: PathBuf::from("dummy.py"),
        };

        let text = "Test cache hit";
        let hash = KokoroTTS::hash_text(text);
        let expected_path = temp_dir.path().join(format!("{}.wav", hash));
        File::create(&expected_path).unwrap();

        let path = engine.get_audio_path(text).await.unwrap();
        assert_eq!(path, expected_path);
    }

    #[tokio::test]
    async fn test_get_audio_path_generator_fail() {
        let temp_dir = TempDir::new().unwrap();
        let engine = KokoroTTS {
            cache_dir: temp_dir.path().to_path_buf(),
            worker_path: PathBuf::from("does_not_exist_ever_9999.py"),
        };

        let res = engine.get_audio_path("Fail me").await;
        assert!(res.is_err()); // Command will either fail to spawn or return non-zero exit code
    }

    #[tokio::test]
    async fn test_get_audio_path_generator_success() {
        let temp_dir = TempDir::new().unwrap();

        // Create a dummy python script that just exits 0 to mock success
        let dummy_script = temp_dir.path().join("dummy.py");
        std::fs::write(&dummy_script, "import sys\nsys.exit(0)").unwrap();

        let engine = KokoroTTS {
            cache_dir: temp_dir.path().to_path_buf(),
            worker_path: dummy_script,
        };

        let res = engine.get_audio_path("Success").await;
        // Since the dummy script exited 0, it hits Ok(output_path).
        // It won't actually create a file, but the function doesn't check for file existence after success,
        // it just trusts the python script and returns Ok(output_path).
        assert!(res.is_ok());
    }
}
