//! Message building utilities for Telegram bot responses

use crate::utils::{format_size, format_subtitle_locales};

/// Build completion message for service uploads (Gofile, Buzzheavier, Pixeldrain)
pub fn build_service_completion_message(
    filename: &str,
    size: u64,
    audio_locale: Option<&str>,
    subtitle_locales: &[String],
    upload_duration: f64,
    upload_speed: f64,
    service_name: &str,
    download_link: &str,
) -> String {
    format!(
        "✅ Tải xuống hoàn tất!\n\n\
        🗂 File: {}\n\
        📦 Kích thước: {}\n\
        🔊 Âm thanh: {}\n\
        📝 Phụ đề: {}\n\n\
        ⏱ Tải lên: {:.1}s @ {:.1} Mbps\n\n\
        🔗 Liên kết {}:\n{}\n\n\
        💡 Phụ đề đã được nhúng sẵn. Để xem phụ đề, hãy tải về và mở bằng VLC hoặc MX Player.",
        filename,
        format_size(size),
        audio_locale.unwrap_or("Không rõ"),
        format_subtitle_locales(subtitle_locales),
        upload_duration,
        upload_speed,
        service_name,
        download_link
    )
}

/// Build completion message for Telegram uploads
pub fn build_telegram_completion_message(
    filename: &str,
    size: u64,
    audio_locale: Option<&str>,
    subtitle_locales: &[String],
) -> String {
    format!(
        "✅ Tải xuống hoàn tất!\n\n\
        🗂 File: {}\n\
        📦 Kích thước: {}\n\
        🔊 Âm thanh: {}\n\
        📝 Phụ đề: {}\n\n\
        💡 Phụ đề đã được nhúng sẵn. Để xem phụ đề, hãy tải về và mở bằng VLC hoặc MX Player.",
        filename,
        format_size(size),
        audio_locale.unwrap_or("Không rõ"),
        format_subtitle_locales(subtitle_locales),
    )
}

/// Build cache hit message with file details
pub fn build_cache_hit_message(
    filename: &str,
    size_mb: f64,
    audio: &str,
    subtitles: &str,
    service_name: Option<&str>,
    download_url: Option<&str>,
) -> String {
    match (service_name, download_url) {
        (Some(service), Some(url)) => {
            format!(
                "✅ Tìm thấy trong cache!\n\n\
                🗂 Tập tin: {}\n\
                📦 Kích thước: {:.2} MB\n\
                🔊 Âm thanh: {}\n\
                📝 Phụ đề: {}\n\n\
                🔗 Liên kết {}:\n{}\n\n\
                💡 Phụ đề đã được nhúng sẵn. Để xem phụ đề, hãy tải về và mở bằng VLC hoặc MX Player.",
                filename, size_mb, audio, subtitles, service, url
            )
        }
        _ => {
            format!(
                "🎞 Đã gửi từ cache!\n\n\
                🗂 Tập tin: {}\n\
                📦 Kích thước: {:.2} MB\n\
                🔊 Âm thanh: {}\n\
                📝 Phụ đề: {}\n\n\
                💡 Phụ đề đã được nhúng sẵn. Để xem phụ đề, hãy tải về và mở bằng VLC hoặc MX Player.",
                filename, size_mb, audio, subtitles
            )
        }
    }
}

/// Build subscriber notification message when download completes
pub fn build_subscriber_notification(
    filename: &str,
    size: u64,
    service_name: Option<&str>,
    download_link: Option<&str>,
) -> String {
    match (service_name, download_link) {
        (Some(service), Some(url)) => {
            format!(
                "🎉 Tập tin bạn yêu cầu đã sẵn sàng!\n\n\
                🗂 Tập tin: {}\n\
                📦 Kích thước: {:.2} MB\n\n\
                🔗 Liên kết {}:\n{}",
                filename,
                size as f64 / 1_048_576.0,
                service,
                url
            )
        }
        _ => {
            format!(
                "🎉 Tập tin bạn yêu cầu đã sẵn sàng!\n\n\
                🗂 Tập tin: {}\n\n\
                📤 Tập tin đã được gửi đến cuộc trò chuyện.",
                filename
            )
        }
    }
}

/// Build error message for download failures
pub fn build_error_message(error: &str) -> String {
    format!("❌ Tải xuống thất bại!\n\n⚠️ Lỗi: {}\n\n🔄 Vui lòng thử lại sau.", error)
}

/// Build waiting in queue message
pub fn build_queue_message(
    title: &str,
    phase: &str,
    progress: u8,
    position: usize,
) -> String {
    format!(
        "⏳ Tập phim này đã được yêu cầu tải xuống bởi một người dùng khác.\n\
        📤 Sẽ gửi cho bạn khi tập phim này sẵn sàng.\n\n\
        🎬 Tiêu đề: {}\n\
        📊 Trạng thái: {}\n\
        📈 Tiến độ: {}%\n\n\
        👥 Bạn là người thứ #{} trong hàng đợi.",
        title, phase, progress, position
    )
}
