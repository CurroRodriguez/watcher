use std::path::PathBuf;

use clap::Parser;

/// CLI arguments parsed by clap.
#[derive(Parser, Debug)]
#[command(
    name = "iwatchr",
    about = "Watch directories and run a command on every file change",
    long_about = None,
)]
pub struct Args {
    /// Directories to watch (positional). When --exec is omitted the last
    /// positional argument is treated as the command to run.
    pub paths: Vec<String>,

    /// Additional directory to watch (repeatable).
    #[arg(long = "watch", short = 'w', value_name = "DIR")]
    pub watch: Vec<PathBuf>,

    /// Command to execute on file change.
    #[arg(long = "exec", short = 'e', value_name = "CMD")]
    pub exec: Option<String>,

    /// Debounce delay in milliseconds before the command is triggered.
    #[arg(long, default_value_t = 500, value_name = "MS")]
    pub debounce: u64,

    /// Glob pattern to ignore (repeatable). `.git/**` is always ignored.
    #[arg(long = "ignore", short = 'i', value_name = "PATTERN")]
    pub ignore: Vec<String>,

    /// Print version information and exit.
    #[arg(short = 'v', long = "version")]
    pub version: bool,
}

/// Validated, resolved configuration ready for use by the watcher and runner.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub dirs: Vec<PathBuf>,
    pub command: String,
    pub debounce_ms: u64,
    pub ignore_patterns: Vec<String>,
}

impl Args {
    /// Merge positional paths with `--watch` flags and extract the command.
    ///
    /// Rules:
    /// - If `--exec` is provided, all positional args are treated as directories.
    /// - Otherwise, the **last** positional arg is the command; the rest are dirs.
    /// - `.git/**` is always prepended to the ignore list.
    pub fn resolve(self) -> Result<Config, String> {
        let (dirs, command) = if let Some(cmd) = self.exec {
            let dirs = self.paths.into_iter().map(PathBuf::from).collect();
            (dirs, cmd)
        } else {
            let mut positionals = self.paths;
            if positionals.is_empty() {
                return Err(
                    "No command provided. Pass it as the last positional argument or use --exec."
                        .into(),
                );
            }
            let command = positionals.pop().unwrap();
            let dirs = positionals.into_iter().map(PathBuf::from).collect();
            (dirs, command)
        };

        let mut all_dirs: Vec<PathBuf> = dirs;
        all_dirs.extend(self.watch);

        if all_dirs.is_empty() {
            return Err(
                "No directories to watch. Provide at least one path or use --watch <DIR>.".into(),
            );
        }

        let mut ignore_patterns = vec![".git/**".to_string()];
        ignore_patterns.extend(self.ignore);

        Ok(Config {
            dirs: all_dirs,
            command,
            debounce_ms: self.debounce,
            ignore_patterns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("iwatchr").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn positional_last_arg_is_command() {
        let config = parse(&["./src", "cargo test"]).resolve().unwrap();
        assert_eq!(config.command, "cargo test");
        assert_eq!(config.dirs, vec![PathBuf::from("./src")]);
    }

    #[test]
    fn exec_flag_takes_precedence() {
        let config = parse(&["--exec", "cargo test", "./src"])
            .resolve()
            .unwrap();
        assert_eq!(config.command, "cargo test");
        assert_eq!(config.dirs, vec![PathBuf::from("./src")]);
    }

    #[test]
    fn multiple_positional_dirs_with_command() {
        let config = parse(&["./src", "./lib", "echo hi"]).resolve().unwrap();
        assert_eq!(
            config.dirs,
            vec![PathBuf::from("./src"), PathBuf::from("./lib")]
        );
        assert_eq!(config.command, "echo hi");
    }

    #[test]
    fn watch_flag_merges_with_positional_dirs() {
        let config = parse(&["./src", "--watch", "./lib", "echo hi"])
            .resolve()
            .unwrap();
        assert_eq!(
            config.dirs,
            vec![PathBuf::from("./src"), PathBuf::from("./lib")]
        );
    }

    #[test]
    fn watch_flag_only_with_exec() {
        let config = parse(&["--watch", "./src", "--exec", "make"])
            .resolve()
            .unwrap();
        assert_eq!(config.dirs, vec![PathBuf::from("./src")]);
        assert_eq!(config.command, "make");
    }

    #[test]
    fn default_debounce_is_500ms() {
        let config = parse(&["./src", "echo hi"]).resolve().unwrap();
        assert_eq!(config.debounce_ms, 500);
    }

    #[test]
    fn custom_debounce_is_respected() {
        let config = parse(&["--debounce", "200", "./src", "echo hi"])
            .resolve()
            .unwrap();
        assert_eq!(config.debounce_ms, 200);
    }

    #[test]
    fn git_pattern_always_in_ignore_list() {
        let config = parse(&["./src", "echo hi"]).resolve().unwrap();
        assert!(config.ignore_patterns.contains(&".git/**".to_string()));
    }

    #[test]
    fn user_ignore_patterns_are_appended() {
        let config = parse(&["--ignore", "**/*.tmp", "--ignore", "dist/**", "./src", "echo hi"])
            .resolve()
            .unwrap();
        assert!(config.ignore_patterns.contains(&".git/**".to_string()));
        assert!(config.ignore_patterns.contains(&"**/*.tmp".to_string()));
        assert!(config.ignore_patterns.contains(&"dist/**".to_string()));
    }

    #[test]
    fn error_when_no_command_and_no_exec() {
        // Zero positional args, no --exec
        let args = Args::try_parse_from(["iwatchr"]).unwrap();
        assert!(args.resolve().is_err());
    }

    #[test]
    fn error_when_no_dirs() {
        // --exec provided but no dirs at all
        let args = parse(&["--exec", "cargo test"]);
        assert!(args.resolve().is_err());
    }

    #[test]
    fn error_message_mentions_exec_or_positional() {
        let args = Args::try_parse_from(["iwatchr"]).unwrap();
        let err = args.resolve().unwrap_err();
        assert!(err.contains("--exec") || err.contains("positional"));
    }

    // ── Version / help flags ────────────────────────────────────────────────

    #[test]
    fn version_flag_short_sets_field() {
        let args = Args::try_parse_from(["iwatchr", "-v"]).unwrap();
        assert!(args.version);
    }

    #[test]
    fn version_flag_long_sets_field() {
        let args = Args::try_parse_from(["iwatchr", "--version"]).unwrap();
        assert!(args.version);
    }

    #[test]
    fn version_flag_is_false_by_default() {
        let args = Args::try_parse_from(["iwatchr", "./src", "echo hi"]).unwrap();
        assert!(!args.version);
    }

    #[test]
    fn help_flag_short_is_recognised() {
        let err = Args::try_parse_from(["iwatchr", "-h"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn help_flag_long_is_recognised() {
        let err = Args::try_parse_from(["iwatchr", "--help"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }
}