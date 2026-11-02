#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if moomole_lib::run_mole_cleanup_worker() {
        return;
    }
    if moomole_lib::run_native_video_worker() {
        return;
    }
    if moomole_lib::run_native_raw_worker() {
        return;
    }
    if moomole_lib::run_native_live_photo_worker() {
        return;
    }
    // A missing/unsupported internal worker route must never fall through to
    // starting a GUI or opening the normal application database.
    if std::env::args_os()
        .skip(1)
        .any(|a| a.to_string_lossy().starts_with("--moomole-"))
    {
        eprintln!("Unsupported internal worker request");
        std::process::exit(2);
    }
    moomole_lib::run()
}
