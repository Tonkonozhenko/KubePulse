//! Local files: layout and cluster display settings. No secrets are stored.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Where this application keeps its files.
#[derive(Clone, Debug)]
pub struct ConfigPaths {
    pub workspace: PathBuf,
    pub clusters: PathBuf,
}

impl ConfigPaths {
    pub fn in_directory(directory: &Path) -> Self {
        Self {
            workspace: directory.join("workspace.json"),
            clusters: directory.join("clusters.json"),
        }
    }

    /// The platform's configuration directory.
    pub fn platform_default() -> Option<Self> {
        let directories = directories::ProjectDirs::from("", "", "KubePulse")?;
        Some(Self::in_directory(directories.config_dir()))
    }
}

/// Replaces `path` in one step, so a crash cannot leave half a file.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, path)
}

/// Reads and parses a settings file. A missing or damaged file means defaults,
/// because settings must never stop the application starting.
pub fn read_or_default<T: Default>(
    path: &Path,
    parse: impl FnOnce(&str) -> serde_json::Result<T>,
) -> T {
    let Ok(text) = fs::read_to_string(path) else {
        return T::default();
    };
    parse(&text).unwrap_or_else(|error| {
        tracing::warn!(path = %path.display(), %error, "ignoring damaged settings file");
        T::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_create_the_directory_and_replace_the_old_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("file.json");
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn missing_and_damaged_files_mean_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file.json");
        let parse = |text: &str| serde_json::from_str::<Vec<u32>>(text);
        assert!(read_or_default(&path, parse).is_empty());

        fs::write(&path, "not json").unwrap();
        assert!(read_or_default(&path, parse).is_empty());

        fs::write(&path, "[1]").unwrap();
        assert_eq!(read_or_default(&path, parse), [1]);
    }
}
