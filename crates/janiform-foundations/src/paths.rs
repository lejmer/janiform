//! Shared path manipulation helpers.

use std::path::Path;
use std::path::PathBuf;

/// Resolve a path-like value against a base directory unless it is already absolute.
pub fn resolve_path_relative_to(base_directory: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);

    if path.is_absolute() {
        return path.to_path_buf();
    }

    base_directory.join(path)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::resolve_path_relative_to;

    #[test]
    fn resolve_path_relative_to_joins_relative_path() {
        assert_eq!(
            resolve_path_relative_to(Path::new("run"), "artifacts/model.json"),
            Path::new("run").join("artifacts/model.json")
        );
    }

    #[test]
    fn resolve_path_relative_to_preserves_absolute_path() {
        let absolute_path = if cfg!(windows) {
            Path::new("C:\\janiform\\model.json")
        } else {
            Path::new("/janiform/model.json")
        };

        assert_eq!(
            resolve_path_relative_to(Path::new("run"), &absolute_path.display().to_string()),
            absolute_path
        );
    }
}
