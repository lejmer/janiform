//! Shared path manipulation helpers.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Component;
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

/// Render a path relative to the process working directory with portable separators.
pub fn format_path(path: &Path) -> String {
    match std::env::current_dir() {
        Ok(current_directory) => format_path_relative_to(current_directory.as_path(), path),
        Err(_) => normalize_path_separators(path),
    }
}

/// Render a path relative to a base directory with portable separators.
pub fn format_path_relative_to(base_directory: &Path, path: &Path) -> String {
    if let Ok(relative_path) = path.strip_prefix(base_directory) {
        return normalize_path_separators(relative_path);
    }

    if path.is_absolute()
        && base_directory.is_absolute()
        && let Some(relative_path) = lexical_relative_path(base_directory, path)
    {
        return normalize_path_separators(relative_path.as_path());
    }

    normalize_path_separators(path)
}

/// Render an optional path-like value relative to a base directory with portable separators.
pub fn format_optional_path_relative_to(
    base_directory: &Path,
    path: Option<&str>,
) -> Option<String> {
    path.map(|path| format_path_relative_to(base_directory, Path::new(path)))
}

/// Render a path with `/` separators.
pub fn normalize_path_separators(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        return ".".to_string();
    }

    let rendered = path.display().to_string();

    if std::path::MAIN_SEPARATOR == '/' {
        rendered
    } else {
        rendered.replace(std::path::MAIN_SEPARATOR, "/")
    }
}

fn lexical_relative_path(base_directory: &Path, path: &Path) -> Option<PathBuf> {
    let base_parts = split_lexical_path(base_directory);
    let path_parts = split_lexical_path(path);

    if base_parts.root != path_parts.root {
        return None;
    }

    let common_prefix_length = base_parts
        .segments
        .iter()
        .zip(path_parts.segments.iter())
        .take_while(|(left, right)| left == right)
        .count();

    let mut relative_path = PathBuf::new();

    for _ in common_prefix_length..base_parts.segments.len() {
        relative_path.push("..");
    }

    for segment in path_parts.segments.iter().skip(common_prefix_length) {
        relative_path.push(segment);
    }

    if relative_path.as_os_str().is_empty() {
        relative_path.push(".");
    }

    Some(relative_path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LexicalPathParts {
    root: Vec<OsString>,
    segments: Vec<OsString>,
}

fn split_lexical_path(path: &Path) -> LexicalPathParts {
    let mut root = Vec::new();
    let mut segments: Vec<OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => root.push(prefix.as_os_str().to_os_string()),
            Component::RootDir => root.push(component.as_os_str().to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => push_parent_segment(&mut segments),
            Component::Normal(segment) => segments.push(segment.to_os_string()),
        }
    }

    LexicalPathParts { root, segments }
}

fn push_parent_segment(segments: &mut Vec<OsString>) {
    match segments.last() {
        Some(segment) if segment != OsStr::new("..") => {
            segments.pop();
        }
        _ => segments.push(OsString::from("..")),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        format_optional_path_relative_to, format_path, format_path_relative_to,
        normalize_path_separators, resolve_path_relative_to,
    };

    #[test]
    fn resolve_path_relative_to_joins_relative_path() {
        assert_eq!(
            resolve_path_relative_to(Path::new("run"), "artifacts/model.json"),
            Path::new("run").join("artifacts/model.json")
        );
    }

    #[test]
    fn resolve_path_relative_to_preserves_absolute_path() {
        let rooted_path = if cfg!(windows) {
            Path::new("C:\\janiform\\model.json")
        } else {
            Path::new("/janiform/model.json")
        };

        assert_eq!(
            resolve_path_relative_to(Path::new("run"), &normalize_path_separators(rooted_path)),
            rooted_path
        );
    }

    #[test]
    fn format_path_relative_to_strips_base_directory_and_normalizes_separators() {
        assert_eq!(
            format_path_relative_to(
                Path::new("run"),
                &Path::new("run").join("artifacts").join("model.json"),
            ),
            "artifacts/model.json"
        );
    }

    #[test]
    fn format_path_uses_process_working_directory_as_display_root() {
        let path = std::env::current_dir()
            .expect("current directory should be readable")
            .join("artifacts")
            .join("model.json");

        assert_eq!(format_path(path.as_path()), "artifacts/model.json");
    }

    #[test]
    fn normalize_path_separators_uses_forward_slashes() {
        assert_eq!(
            normalize_path_separators(&Path::new("run").join("artifacts").join("model.json")),
            "run/artifacts/model.json"
        );
    }

    #[test]
    fn format_optional_path_relative_to_formats_present_path() {
        assert_eq!(
            format_optional_path_relative_to(Path::new("run"), Some("run/artifacts/model.json"),),
            Some("artifacts/model.json".to_string())
        );
    }
}
