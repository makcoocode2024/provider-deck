use std::{fs, io::Write};

use chrono::Utc;
use directories::ProjectDirs;
use serde_json::json;

pub fn record(action: &str, detail: &str, success: bool) {
    let Some(dirs) = ProjectDirs::from("cn", "ProviderDeck", "Provider Deck") else { return; };
    let dir = dirs.data_dir().join("logs");
    if fs::create_dir_all(&dir).is_err() { return; }
    let path = dir.join("operations.jsonl");
    let entry = json!({
        "timestamp": Utc::now().to_rfc3339(),
        "action": action,
        "detail": detail,
        "success": success,
    });
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{entry}");
    }
}
