use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

/// A background app with no console has nowhere else for diagnostics to go,
/// so failures get appended here instead of silently vanishing.
fn log_path() -> std::path::PathBuf {
    crate::config::data_dir().join("Wallboxd.log")
}

fn write_line(level: &str, msg: &str) {
    // Callers span multiple threads (main thread, refresh threads, the
    // autostart thread) writing to the same file concurrently. `writeln!`
    // issues several small `write` calls per line (one per literal/argument
    // piece), so two threads' calls can interleave mid-line even though each
    // individual `WriteFile` with FILE_APPEND_DATA is itself atomic -
    // confirmed by observing corrupted/merged lines under concurrent writes
    // during testing. Formatting into one `String` first and writing it in
    // a single `write_all`, serialized by this mutex, makes each log line
    // atomic regardless of how many threads are logging at once.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock();

    let _ = std::fs::create_dir_all(crate::config::data_dir());
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    else {
        return;
    };
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("[{secs}] {level}: {msg}\n");
    let _ = file.write_all(line.as_bytes());
}

pub fn info(msg: &str) {
    write_line("INFO", msg);
}

pub fn error(msg: &str) {
    write_line("ERROR", msg);
}
