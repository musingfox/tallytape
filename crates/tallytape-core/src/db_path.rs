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

    #[test]
    fn parent_contains_tallytape_segment() {
        let path = db_path().expect("db_path should resolve");
        let parent_str = path.parent().unwrap().to_string_lossy().to_string();
        assert!(
            parent_str.contains("tallytape"),
            "parent path should contain 'tallytape', got: {parent_str}"
        );
    }

    #[test]
    fn log_path_returns_writer_log_filename() {
        let path = log_path().expect("log_path should resolve");
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some("writer.log")
        );
    }

    #[test]
    fn log_path_parent_is_same_as_db_path_parent() {
        let log = log_path().expect("log_path should resolve");
        let db = db_path().expect("db_path should resolve");
        assert_eq!(log.parent(), db.parent());
    }
}
