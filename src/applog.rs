use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

/// A background app with no console has nowhere else for diagnostics to go,
/// so failures get appended here instead of silently vanishing.
fn log_path() -> std::path::PathBuf {
    crate::config::data_dir().join("wallb.log")
}

fn write_line(level: &str, msg: &str) {
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
    let _ = writeln!(file, "[{secs}] {level}: {msg}");
}

pub fn info(msg: &str) {
    write_line("INFO", msg);
}

pub fn error(msg: &str) {
    write_line("ERROR", msg);
}
