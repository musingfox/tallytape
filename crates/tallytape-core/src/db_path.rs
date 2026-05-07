use std::path::PathBuf;

use directories::ProjectDirs;

pub fn db_path() -> anyhow::Result<PathBuf> {
    let proj = ProjectDirs::from("", "", "tallytape")
        .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory"))?;
    let dir = proj.data_local_dir().to_path_buf();
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("tallytape.sqlite"))
}

pub fn log_path() -> anyhow::Result<PathBuf> {
    let proj = ProjectDirs::from("", "", "tallytape")
        .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory"))?;
    let dir = proj.data_local_dir().to_path_buf();
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("writer.log"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[serial_test::serial]
    #[test]
    fn returns_path_ending_in_sqlite_file() {
        let path = db_path().expect("db_path should resolve");
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some("tallytape.sqlite")
        );
        let parent = path.parent().expect("path has parent");
        assert!(parent.exists(), "parent dir should be created");
        assert!(parent.is_dir());
    }

    #[serial_test::serial]
    #[test]
    fn parent_contains_tallytape_segment() {
        let path = db_path().expect("db_path should resolve");
        let parent_str = path.parent().unwrap().to_string_lossy().to_string();
        assert!(
            parent_str.contains("tallytape"),
            "parent path should contain 'tallytape', got: {parent_str}"
        );
    }

    #[serial_test::serial]
    #[test]
    fn log_path_returns_writer_log_filename() {
        let path = log_path().expect("log_path should resolve");
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some("writer.log")
        );
    }

    #[serial_test::serial]
    #[test]
    fn log_path_parent_is_same_as_db_path_parent() {
        let log = log_path().expect("log_path should resolve");
        let db = db_path().expect("db_path should resolve");
        assert_eq!(log.parent(), db.parent());
    }

    /// T5: log_path() with a fresh HOME tempdir creates the parent directory.
    #[serial_test::serial]
    #[test]
    fn log_path_creates_parent_dir_with_fresh_home() {
        use tempfile::tempdir;
        let home = tempdir().unwrap();
        // Set HOME to the fresh tempdir so ProjectDirs resolves under it.
        std::env::set_var("HOME", home.path());
        let path = log_path().expect("log_path should resolve with fresh HOME");
        let parent = path.parent().expect("path has parent");
        assert!(
            parent.exists(),
            "parent dir should be created by log_path(), but {} does not exist",
            parent.display()
        );
        assert!(parent.is_dir());
        // Restore HOME (best-effort; tests may run in parallel so this is advisory)
        std::env::remove_var("HOME");
    }
}
