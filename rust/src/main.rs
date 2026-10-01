use clap::Parser;

use openloop::cli::{self, Cli};
use openloop::tui;

fn main() {
    // Launch the interactive TUI when stdout is a TTY and no args are given.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() && cli::stdout_is_tty() {
        match tui::run() {
            Ok(()) => {}
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }

    let cli = Cli::parse();
    match cli::dispatch(cli.command) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
