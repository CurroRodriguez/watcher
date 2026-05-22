use std::path::PathBuf;
use std::sync::mpsc::Sender;

use globset::{Glob, GlobSet, GlobSetBuilder};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Builds a [`GlobSet`] from a list of glob pattern strings.
///
/// Patterns that do not begin with `**/` or `/` are automatically prefixed
/// with `**/` so they match anywhere in an absolute path (as returned by
/// the filesystem watcher).
pub fn build_ignore_set(patterns: &[String]) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let normalized = normalize_pattern(pattern);
        builder.add(Glob::new(&normalized)?);
    }
    builder.build()
}

/// Ensures a pattern can match anywhere in an absolute path.
///
/// Patterns without an anchor are prefixed with `**/` so they match anywhere
/// in the absolute paths that OS watchers return. The following are left
/// unchanged:
/// - Already anchored: `**/foo`
/// - Unix absolute: `/abs/path`
/// - Windows absolute: `C:\path` or `C:/path` (drive-letter prefix)
fn normalize_pattern(pattern: &str) -> String {
    if pattern.starts_with("**/") || pattern.starts_with('/') || is_windows_absolute(pattern) {
        pattern.to_string()
    } else {
        format!("**/{pattern}")
    }
}

/// Returns `true` if `s` looks like a Windows absolute path (`X:\` or `X:/`).
fn is_windows_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/')
}

/// Returns `true` if `path` should be ignored according to `ignore_set`.
fn is_ignored(path: &PathBuf, ignore_set: &GlobSet) -> bool {
    ignore_set.is_match(path)
}

/// Creates a recursive [`RecommendedWatcher`] that forwards change events to
/// `tx`. Events on paths that match `ignore_set` are silently dropped.
pub fn create_watcher(
    dirs: &[PathBuf],
    ignore_set: GlobSet,
    tx: Sender<()>,
) -> notify::Result<RecommendedWatcher> {
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<Event>| {
            let event = match result {
                Ok(e) => e,
                Err(_) => return,
            };

            // Only react to meaningful change events.
            if !matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                return;
            }

            let any_relevant = event.paths.iter().any(|p| !is_ignored(p, &ignore_set));
            if any_relevant {
                let _ = tx.send(());
            }
        })?;

    for dir in dirs {
        watcher.watch(dir.as_path(), RecursiveMode::Recursive)?;
    }

    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_ignore_set ────────────────────────────────────────────────────

    #[test]
    fn valid_patterns_build_successfully() {
        let result = build_ignore_set(&[".git/**".to_string(), "**/*.tmp".to_string()]);
        assert!(result.is_ok());
    }

    #[test]
    fn invalid_pattern_returns_error() {
        let result = build_ignore_set(&["[invalid_glob".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn empty_pattern_list_builds_empty_set() {
        let set = build_ignore_set(&[]).unwrap();
        assert!(!is_ignored(&PathBuf::from("src/main.rs"), &set));
    }

    // ── normalize_pattern ───────────────────────────────────────────────────

    #[test]
    fn bare_pattern_gets_globstar_prefix() {
        assert_eq!(normalize_pattern(".git/**"), "**/.git/**");
    }

    #[test]
    fn already_prefixed_pattern_is_unchanged() {
        assert_eq!(normalize_pattern("**/*.tmp"), "**/*.tmp");
    }

    #[test]
    fn absolute_pattern_is_unchanged() {
        assert_eq!(normalize_pattern("/abs/path"), "/abs/path");
    }

    #[test]
    fn windows_drive_letter_backslash_is_unchanged() {
        assert_eq!(normalize_pattern(r"C:\project\**"), r"C:\project\**");
    }

    #[test]
    fn windows_drive_letter_forwardslash_is_unchanged() {
        assert_eq!(normalize_pattern("C:/project/**"), "C:/project/**");
    }

    #[test]
    fn is_windows_absolute_recognises_drive_paths() {
        assert!(is_windows_absolute(r"C:\foo"));
        assert!(is_windows_absolute("D:/bar"));
        assert!(!is_windows_absolute("**/foo"));
        assert!(!is_windows_absolute("/unix/abs"));
        assert!(!is_windows_absolute("relative/path"));
    }

    // ── is_ignored ──────────────────────────────────────────────────────────

    #[test]
    fn git_dir_relative_path_is_ignored() {
        let set = build_ignore_set(&[".git/**".to_string()]).unwrap();
        assert!(is_ignored(&PathBuf::from(".git/config"), &set));
        assert!(is_ignored(&PathBuf::from(".git/refs/heads/main"), &set));
    }

    #[test]
    fn git_dir_absolute_path_is_ignored() {
        let set = build_ignore_set(&[".git/**".to_string()]).unwrap();
        // Unix-style absolute path
        assert!(is_ignored(
            &PathBuf::from("/home/user/project/.git/config"),
            &set
        ));
    }

    #[test]
    #[cfg(windows)]
    fn git_dir_windows_absolute_path_is_ignored() {
        let set = build_ignore_set(&[".git/**".to_string()]).unwrap();
        // Windows-style absolute path (PathBuf parses drive letters correctly)
        assert!(is_ignored(
            &PathBuf::from(r"C:\Users\user\project\.git\config"),
            &set
        ));
    }

    #[test]
    fn non_ignored_path_passes_through() {
        let set = build_ignore_set(&[".git/**".to_string()]).unwrap();
        assert!(!is_ignored(&PathBuf::from("src/main.rs"), &set));
    }

    #[test]
    fn tmp_pattern_matches_nested_file() {
        let set = build_ignore_set(&["**/*.tmp".to_string()]).unwrap();
        assert!(is_ignored(&PathBuf::from("build/cache.tmp"), &set));
        assert!(is_ignored(&PathBuf::from("/abs/build/cache.tmp"), &set));
        assert!(!is_ignored(&PathBuf::from("src/main.rs"), &set));
    }

    #[test]
    fn dist_pattern_matches_subdirectory() {
        let set = build_ignore_set(&["dist/**".to_string()]).unwrap();
        assert!(is_ignored(&PathBuf::from("dist/bundle.js"), &set));
        assert!(!is_ignored(&PathBuf::from("src/index.js"), &set));
    }

    #[test]
    fn multiple_patterns_each_applied() {
        let set =
            build_ignore_set(&[".git/**".to_string(), "**/*.tmp".to_string()]).unwrap();
        assert!(is_ignored(&PathBuf::from(".git/config"), &set));
        assert!(is_ignored(&PathBuf::from("build/cache.tmp"), &set));
        assert!(!is_ignored(&PathBuf::from("src/lib.rs"), &set));
    }
}
