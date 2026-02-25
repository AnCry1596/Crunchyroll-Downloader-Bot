/// Format file size for display with appropriate unit (B, KB, MB, GB)
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format bytes to megabytes
pub fn bytes_to_mb(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

/// Format subtitle locales for display
pub fn format_subtitle_locales(locales: &[String]) -> String {
    if locales.is_empty() {
        return "Không có".to_string();
    }
    locales.join(", ")
}

/// Format optional subtitle locales for display
pub fn format_optional_subtitles(locales: Option<&Vec<String>>) -> String {
    match locales {
        Some(l) if !l.is_empty() => l.join(", "),
        _ => "Không có".to_string(),
    }
}

/// Get audio locale with default fallback
pub fn audio_or_default(audio: Option<&str>) -> &str {
    audio.unwrap_or("Không rõ")
}

/// Calculate upload speed in Mbps from bytes and duration
pub fn upload_speed_mbps(bytes: u64, duration_secs: f64) -> f64 {
    if duration_secs > 0.0 {
        bytes_to_mb(bytes) / duration_secs * 8.0
    } else {
        0.0
    }
}

/// Format ETA from seconds into human readable string
pub fn format_eta(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{:.0}s", seconds)
    } else if seconds < 3600.0 {
        let mins = (seconds / 60.0) as u32;
        let secs = (seconds % 60.0) as u32;
        format!("{}:{:02}", mins, secs)
    } else {
        let hours = (seconds / 3600.0) as u32;
        let mins = ((seconds % 3600.0) / 60.0) as u32;
        let secs = (seconds % 60.0) as u32;
        format!("{}:{:02}:{:02}", hours, mins, secs)
    }
}

/// Format owner user IDs for display in messages
pub fn format_owner_ids(owner_ids: &[i64]) -> String {
    if owner_ids.is_empty() {
        return "Không có".to_string();
    }
    owner_ids
        .iter()
        .map(|id| format!("`{}`", id))
        .collect::<Vec<_>>()
        .join(", ")
}
