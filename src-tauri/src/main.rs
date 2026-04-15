#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::IsTerminal;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let force_gui = args.iter().any(|a| a == "--gui");
    let force_tui = args.iter().any(|a| a == "--tui");

    // Default: TUI if stdin is a terminal, GUI otherwise (e.g. launched from desktop)
    let use_gui = force_gui || (!force_tui && !std::io::stdin().is_terminal());

    if use_gui {
        screenshare_lib::run();
    } else {
        if let Err(e) = screenshare_lib::tui::run_tui() {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
