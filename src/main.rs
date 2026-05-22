use clap::Parser;
use iwatchr::cli::Args;
use iwatchr::{runner, watcher};

fn main() {
    let args = Args::parse();

    // Handle -v / --version before resolve() consumes args.
    if args.version {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let config = match args.resolve() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[iwatchr] Error: {e}");
            std::process::exit(1);
        }
    };

    let ignore_set = match watcher::build_ignore_set(&config.ignore_patterns) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[iwatchr] Invalid ignore pattern: {e}");
            std::process::exit(1);
        }
    };

    // Startup banner
    println!("[iwatchr] Watching:");
    for dir in &config.dirs {
        println!("         {}", dir.display());
    }
    println!("[iwatchr] Command  : {}", config.command);
    println!("[iwatchr] Debounce : {}ms", config.debounce_ms);
    let user_ignores: Vec<_> = config
        .ignore_patterns
        .iter()
        .filter(|p| p.as_str() != ".git/**")
        .collect();
    if !user_ignores.is_empty() {
        println!(
            "[iwatchr] Ignoring : {}",
            user_ignores
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!("[iwatchr] Press Ctrl-C to stop.\n");

    let (tx, rx) = std::sync::mpsc::channel();

    ctrlc::set_handler(move || {
        eprintln!("\n[iwatchr] Stopped.");
        std::process::exit(0);
    })
    .expect("Failed to install Ctrl-C handler");

    runner::start(rx, config.command.clone(), config.debounce_ms);

    let _watcher = match watcher::create_watcher(&config.dirs, ignore_set, tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[iwatchr] Failed to start watcher: {e}");
            std::process::exit(1);
        }
    };

    // Keep the main thread alive; the watcher and runner run on other threads.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
