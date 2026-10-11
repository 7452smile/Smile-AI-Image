#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--mcp") {
        if let Err(error) = smile_mcp::run() {
            eprintln!("{error}");
            std::process::exit(1);
        }
    } else {
        smile_ai_image_settings_lib::run();
    }
}
