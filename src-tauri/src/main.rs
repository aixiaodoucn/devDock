#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    if std::env::args_os().skip(1).eq(["--log-mcp-stdio"]) {
        if let Err(message) = dev_dock_lib::log_bridge::run_stdio() {
            eprintln!("{message}");
            std::process::exit(1);
        }
        return;
    }
    dev_dock_lib::run();
}
