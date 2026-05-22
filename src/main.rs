use clap::Parser;
use watchr::cli::Args;
use watchr::{runner, watcher};

fn main() {
    let config = match Args::parse().resolve() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[watchr] Error: {e}");
            std::process::exit(1);
        }
    };

    let ignore_set = match watcher::build_ignore_set(&config.ignore_patterns) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[watchr] Invalid ignore pattern: {e}");
            std::process::exit(1);
        }
    };

    // Startup banner
    println!("[watchr] Watching:");
    for dir in &config.dirs {
        println!("         {}", dir.display());
    }
    println!("[watchr] Command  : {}", config.command);
    println!("[watchr] Debounce : {}ms", config.debounce_ms);
    let user_ignores: Vec<_> = config
        .ignore_patterns
        .iter()
        .filter(|p| p.as_str() != ".git/**")
        .collect();
    if !user_ignores.is_empty() {
        println!(
            "[watchr] Ignoring : {}",
            user_ignores
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!("[watchr] Press Ctrl-C to stop.\n");

    let (tx, rx) = std::sync::mpsc::channel();

    ctrlc::set_handler(move || {
        eprintln!("\n[watchr] Stopped.");
        std::process::exit(0);
    })
    .expect("Failed to install Ctrl-C handler");

    runner::start(rx, config.command.clone(), config.debounce_ms);

    let _watcher = match watcher::create_watcher(&config.dirs, ignore_set, tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[watchr] Failed to start watcher: {e}");
            std::process::exit(1);
        }
    };

    // Keep the main thread alive; the watcher and runner run on other threads.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
