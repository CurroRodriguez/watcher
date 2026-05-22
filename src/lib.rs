// Catch unsupported build targets at compile time with a human-readable message.
#[cfg(not(any(unix, windows)))]
compile_error!(
    "watchr only supports Unix-like systems (Linux, macOS) and Windows. \
     The current target is not recognised."
);

pub mod cli;
pub mod runner;
pub mod watcher;
