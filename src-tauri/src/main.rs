#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--gui") {
        screenshare_lib::run();
    } else {
        if let Err(e) = screenshare_lib::tui::run_tui() {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
