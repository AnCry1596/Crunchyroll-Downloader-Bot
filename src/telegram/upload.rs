use crate::download::progress::SharedProgress;
use crate::download::DownloadPhase;
use crate::error::{Error, Result};
use crate::utils::format_size;
use std::path::Path;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{FileId, InputFile, Message, MessageId, ReplyParameters};
use tokio::fs;

pub const TELEGRAM_MAX_SIZE: u64 = 2000u64 * 1024 * 1024;

pub struct UploadResult {
    pub message: Message,
    pub file_id: String,
}

pub async fn upload_or_link(
    bot: &Bot,
    chat_id: ChatId,
    file_path: &Path,
    filename: &str,
    progress: SharedProgress,
    width: u32,
    height: u32,
) -> Result<Option<UploadResult>> {
    let file_size = fs::metadata(file_path).await?.len();

    if file_size <= TELEGRAM_MAX_SIZE {
        upload_to_telegram(bot, chat_id, file_path, filename, file_size, progress, width, height).await
    } else {
        provide_download_link(bot, chat_id, file_path, file_size, filename).await?;
        Ok(None)
    }
}

async fn upload_to_telegram(
    bot: &Bot,
    chat_id: ChatId,
    file_path: &Path,
    filename: &str,
    file_size: u64,
    progress: SharedProgress,
    width: u32,
    height: u32,
) -> Result<Option<UploadResult>> {
    {
        let mut p = progress.write().await;
        p.upload_total_bytes = file_size;
        p.upload_bytes = 0;
        p.set_phase(DownloadPhase::Uploading { progress: 0.0 });
    }

    let progress_clone = progress.clone();
    let progress_callback: Arc<dyn Fn(teloxide::types::UploadProgress) + Send + Sync> =
        Arc::new(move |upload_progress: teloxide::types::UploadProgress| {
            let progress_inner = progress_clone.clone();
            let bytes_sent = upload_progress.bytes_sent;
            let total = upload_progress.total_bytes.unwrap_or(file_size);

            tokio::spawn(async move {
                let percentage = if total > 0 {
                    (bytes_sent as f32 / total as f32) * 100.0
                } else {
                    0.0
                };

                let mut p = progress_inner.write().await;
                p.upload_total_bytes = total;
                p.update_upload_speed(bytes_sent);
                p.set_phase(DownloadPhase::Uploading { progress: percentage });
            });
        });

    let file = InputFile::file(file_path)
        .file_name(filename.to_string())
        .with_progress(progress_callback);

    let size_str = format_size(file_size);

    let msg = bot
        .send_video(chat_id, file)
        .width(width)
        .height(height)
        .supports_streaming(true)
        .caption(format!("{}\n📦 Kích thước: {}", filename, size_str))
        .send()
        .await
        .map_err(|e| Error::Upload(format!("Failed to upload video: {}", e)))?;

    {
        let mut p = progress.write().await;
        p.upload_bytes = file_size;
        p.set_phase(DownloadPhase::Uploading { progress: 100.0 });
    }
    tracing::info!("Successfully uploaded {} to Telegram", filename);

    let file_id = extract_file_id(&msg);

    Ok(Some(UploadResult {
        message: msg,
        file_id,
    }))
}

fn extract_file_id(msg: &Message) -> String {
    msg.video()
        .map(|v| v.file.id.0.as_str())
        .unwrap_or_default()
        .to_string()
}

pub async fn upload_to_storage(
    bot: &Bot,
    storage_chat_id: ChatId,
    file_path: &Path,
    filename: &str,
    caption: &str,
    progress: SharedProgress,
    width: u32,
    height: u32,
) -> Result<(String, i32, u64)> {
    let file_size = fs::metadata(file_path).await?.len();

    {
        let mut p = progress.write().await;
        p.upload_total_bytes = file_size;
        p.upload_bytes = 0;
        p.set_phase(DownloadPhase::Uploading { progress: 0.0 });
    }

    let progress_clone = progress.clone();
    let progress_callback: Arc<dyn Fn(teloxide::types::UploadProgress) + Send + Sync> =
        Arc::new(move |upload_progress: teloxide::types::UploadProgress| {
            let progress_inner = progress_clone.clone();
            let bytes_sent = upload_progress.bytes_sent;
            let total = upload_progress.total_bytes.unwrap_or(file_size);

            tokio::spawn(async move {
                let percentage = if total > 0 {
                    (bytes_sent as f32 / total as f32) * 100.0
                } else {
                    0.0
                };

                let mut p = progress_inner.write().await;
                p.upload_total_bytes = total;
                p.update_upload_speed(bytes_sent);
                p.set_phase(DownloadPhase::Uploading { progress: percentage });
            });
        });

    let file = InputFile::file(file_path)
        .file_name(filename.to_string())
        .with_progress(progress_callback);

    let msg = bot
        .send_video(storage_chat_id, file)
        .width(width)
        .height(height)
        .supports_streaming(true)
        .caption(caption)
        .send()
        .await
        .map_err(|e| Error::Upload(format!("Failed to upload video to storage: {}", e)))?;

    {
        let mut p = progress.write().await;
        p.upload_bytes = file_size;
        p.set_phase(DownloadPhase::Uploading { progress: 100.0 });
    }

    let file_id = extract_file_id(&msg);
    Ok((file_id, msg.id.0, file_size))
}

pub async fn forward_cached_file(
    bot: &Bot,
    chat_id: ChatId,
    file_id: &str,
    filename: &str,
    file_size: u64,
    reply_to_message_id: Option<MessageId>,
    storage_chat_id: Option<i64>,
    storage_message_id: Option<i32>,
    audio_locale: Option<&str>,
    subtitle_locales: Option<&[String]>,
) -> Result<()> {
    let size_str = format_size(file_size);
    let audio = audio_locale.unwrap_or("Không rõ");
    let subtitles = subtitle_locales
        .map(|s| s.join(", "))
        .unwrap_or_else(|| "Không có".to_string());

    let caption = format!(
        "{}\n\n\
        📦 Kích thước: {}\n\
        🔊 Âm thanh: {}\n\
        📝 Phụ đề: {}\n\n\
        ⚡ (Từ bộ nhớ đệm)",
        filename, size_str, audio, subtitles
    );

    if let (Some(src_chat), Some(src_msg)) = (storage_chat_id, storage_message_id) {
        let mut copy_req = bot.copy_message(chat_id, ChatId(src_chat), MessageId(src_msg))
            .caption(&caption);
        
        if let Some(reply_id) = reply_to_message_id {
            copy_req = copy_req.reply_parameters(ReplyParameters::new(reply_id));
        }

        match copy_req.send().await {
            Ok(_) => return Ok(()),
            Err(e) => {
                tracing::warn!("Copy message failed (maybe deleted from storage), falling back to file_id. Error: {}", e);
            }
        }
    }

    let file = InputFile::file_id(FileId(file_id.to_string()));

    let mut request_video = bot
        .send_video(chat_id, file)
        .supports_streaming(true)
        .caption(&caption);

    if let Some(reply_id) = reply_to_message_id {
        request_video = request_video.reply_parameters(ReplyParameters::new(reply_id));
    }

    request_video
        .send()
        .await
        .map_err(|e| Error::Upload(format!("Failed to forward cached video: {}", e)))?;

    Ok(())
}

async fn provide_download_link(
    bot: &Bot,
    chat_id: ChatId,
    file_path: &Path,
    file_size: u64,
    filename: &str,
) -> Result<()> {
    let size_str = format_size(file_size);

    let message = format!(
        "⚠️ File quá lớn để tải lên Telegram.\n\n📁 Tên file: {}\n📦 Kích thước: {}\n\n💾 File đã được lưu tại:\n{}\n\n💡 Để hỗ trợ file lớn, hãy cấu hình dịch vụ lưu trữ đám mây trong cài đặt bot.",
        filename,
        size_str,
        file_path.display()
    );

    bot.send_message(chat_id, message)
        .await
        .map_err(|e| Error::Telegram(format!("Failed to send message: {}", e)))?;

    Ok(())
}

