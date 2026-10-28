//! Log de disco (sem deps): ~/.config/lenvix/lenvix.log (XDG_CONFIG_HOME).
//! Uma linha por evento: `YYYY-MM-DD HH:MM:SS [NIVEL] mensagem`.
//! Append por escrita (open/append/close) — thread-safe o suficiente para uso
//! serial da engine; qualquer erro de I/O é silencioso (log não deve derrubar app).

use std::{io::Write, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

pub fn log_file() -> PathBuf {
    crate::core::settings::dir().join("lenvix.log")
}

pub fn log(level: &str, msg: &str) {
    let path = log_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = format!("{} [{level}] {msg}\n", timestamp());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// ISO 8601 local simplificado (sem deps: algoritmo civil_from_days)
fn timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let days = (secs / 86_400) as i64;
    let secs_of_day = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Howard Hinnant's civil_from_days (dias desde época → data)
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
