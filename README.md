# watchr

A lightweight, cross-platform CLI file watcher. Point it at one or more directories and it will re-run a command every time a file change is detected.

```
watchr ./src "cargo test"
```

---

## Features

- **Multi-directory** — watch any number of directories at once
- **Recursive** — subdirectories are watched automatically
- **Debounced** — rapid saves are coalesced into a single run (configurable delay)
- **Skip-if-running** — if the previous command is still executing, the new trigger is silently dropped
- **Glob ignore patterns** — filter out files you don't care about (`.git/**` is always ignored)
- **Cross-platform** — Linux, macOS, and Windows

---

## Installation

### From source

Requires [Rust](https://rustup.rs/) 1.85 or later (edition 2024).

```sh
git clone https://github.com/CurroRodriguez/watcher.git
cd watcher
cargo install --path .
```

### Cargo

```sh
cargo install watchr
```

---

## Usage

```
watchr [OPTIONS] [PATHS]... [COMMAND]
```

The simplest form passes directories and the command as positional arguments. The **last** positional argument is always treated as the command to run:

```sh
watchr ./src "cargo test"
watchr ./src ./lib "cargo build"
```

Use `--exec` when the command contains multiple positional-looking words or when you prefer explicit flags:

```sh
watchr --watch ./src --exec "cargo test"
watchr --watch ./src --watch ./lib --exec "make all"
```

You can mix positional directories with `--watch` flags:

```sh
watchr ./src --watch ./assets "npm run build"
```

---

## Options

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--watch <DIR>` | `-w` | — | Additional directory to watch (repeatable) |
| `--exec <CMD>` | `-e` | — | Command to run on change (overrides last positional) |
| `--debounce <MS>` | — | `500` | Milliseconds to wait after the last event before running |
| `--ignore <PATTERN>` | `-i` | — | Glob pattern to ignore (repeatable). `.git/**` is always ignored |
| `--help` | `-h` | — | Print help |
| `--version` | `-V` | — | Print version |

---

## Examples

### Run tests on every source change

```sh
watchr ./src "cargo test"
```

### Rebuild on changes in multiple directories

```sh
watchr --watch ./src --watch ./lib --exec "cargo build"
```

### Faster debounce for interactive workflows

```sh
watchr --debounce 100 ./src "cargo check"
```

### Ignore generated and temporary files

```sh
watchr --ignore "**/*.log" --ignore "dist/**" ./src "npm run build"
```

### Watch the current directory

```sh
watchr . "make"
```

---

## Shell used to run commands

| Platform | Shell |
|----------|-------|
| Linux / macOS | `sh -c <command>` |
| Windows | `powershell -NoProfile -NonInteractive -Command <command>` |

Write commands in the syntax of the shell for your platform. On Windows this means PowerShell syntax; on Unix, standard POSIX shell syntax.

---

## How it works

1. **Watcher** — uses the OS-native filesystem notification API ([`notify`](https://crates.io/crates/notify)) to receive `Create`, `Modify`, and `Remove` events recursively.
2. **Ignore filter** — event paths are matched against the compiled glob set ([`globset`](https://crates.io/crates/globset)). Ignored paths are dropped before they reach the runner.
3. **Debounce** — events flow into a channel. A background thread resets a timer on each event; when the timer expires without a new event the command fires.
4. **Skip-if-running** — an atomic flag prevents a second invocation from starting while the first is still in progress.

---

## License

MIT
