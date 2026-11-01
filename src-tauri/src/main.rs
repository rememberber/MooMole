#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if moomole_lib::run_mole_cleanup_worker() {
        return;
    }
    if moomole_lib::run_native_video_worker() {
        return;
    }
    moomole_lib::run()
}
