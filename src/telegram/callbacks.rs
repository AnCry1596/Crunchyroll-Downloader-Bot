use crate::config::PreferredUploadService;
use crate::database::models::{ActiveDownload, CachedBuzzheavierFile, CachedGofileFile, CachedFile, CachedPixeldrainFile, KeyPair};
use crate::download::progress::new_progress;
use crate::download::DownloadTask;
use crate::telegram::bot::{BotState, DownloadSubscriber};
use crate::telegram::buzzheavier::upload_to_buzzheavier;
use crate::telegram::commands::build_callback_not_authorized;
use crate::telegram::gofile::upload_to_gofile;
use crate::crunchyroll::types::{AudioVersionInfo, Version};
use crate::telegram::bot::AudioSelectionState;
use crate::telegram::keyboards::{
    audio_selection_keyboard, download_complete_keyboard, download_progress_keyboard,
    episode_actions_keyboard_with_pixeldrain, episodes_keyboard, seasons_keyboard,
};
use crate::telegram::pixeldrain::upload_to_pixeldrain;
use crate::telegram::upload::{forward_cached_file, upload_or_link, upload_to_storage};
use crate::utils::{
    new_upload_progress, format_size, bytes_to_mb, format_optional_subtitles, audio_or_default,
    build_service_completion_message, build_telegram_completion_message,
    build_cache_hit_message, build_queue_message, build_subscriber_notification,
    build_error_message, send_completion_message, validate_callback_user,
};
use crate::crunchyroll::types::SubtitleTrack;
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, MessageId};

pub type HandlerResult = ResponseResult<()>;

/// Compare cached subtitle locales with fresh ones from API
/// Returns true if they match (cache is valid)
fn subtitles_match(
    cached_locales: Option<&Vec<String>>,
    fresh_subtitles: &HashMap<String, SubtitleTrack>,
    fresh_captions: &HashMap<String, SubtitleTrack>,
) -> bool {
    let mut fresh_locales: Vec<String> = fresh_subtitles.keys()
        .chain(fresh_captions.keys())
        .cloned()
        .collect();
    fresh_locales.sort();
    fresh_locales.dedup();

    match cached_locales {
        Some(cached) => {
            let mut cached_sorted = cached.clone();
            cached_sorted.sort();
            cached_sorted == fresh_locales
        }
        None => {
            // No cached subtitle info - treat as stale to be safe
            false
        }
    }
}

/// Check subtitle freshness for a cached episode. Returns true if fresh (cache OK).
async fn check_subtitle_freshness(
    state: &BotState,
    episode_id: &str,
    cached_subtitle_locales: Option<&Vec<String>>,
) -> bool {
    match state.cr_client.get_playback(episode_id).await {
        Ok(playback) => subtitles_match(
            cached_subtitle_locales,
            &playback.subtitles,
            &playback.captions,
        ),
        Err(e) => {
            tracing::warn!("Failed to check subtitle freshness for {}: {}. Serving cache anyway.", episode_id, e);
            true // On error, serve cache rather than failing
        }
    }
}

/// Invalidate all cache entries for an episode
async fn invalidate_all_caches(state: &BotState, episode_id: &str) {
    if let Some(ref db) = state.database {
        let _ = db.delete_cached_file(episode_id).await;
        let _ = db.delete_cached_buzzheavier(episode_id).await;
        let _ = db.delete_cached_pixeldrain(episode_id).await;
        let _ = db.delete_cached_gofile(episode_id).await;
        tracing::info!("Invalidated all caches for episode {}", episode_id);
    }
}

pub async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    state: Arc<BotState>,
) -> HandlerResult {
    let data = q.data.as_deref().unwrap_or_default();
    let parts: Vec<&str> = data.split(':').collect();

    // Get the user who clicked the button
    let callback_user_id = q.from.id.0 as i64;

    let chat_id = match q.message.as_ref() {
        Some(msg) => msg.chat().id,
        None => return Ok(()),
    };

    let msg_id = match q.message.as_ref() {
        Some(msg) => msg.id(),
        None => return Ok(()),
    };

    // Check if user is allowed to use the bot in this chat
    if !state.is_allowed(callback_user_id, chat_id.0).await {
        let not_authorized_msg = build_callback_not_authorized(&state.config.telegram.owner_users);
        bot.answer_callback_query(q.id.clone())
            .text(not_authorized_msg)
            .show_alert(true)
            .await?;
        return Ok(());
    }

    // Validate that this callback is for this user (check last part is user_id)
    // Skip validation for "noop" buttons
    if data != "noop" {
        if validate_callback_user(&parts, callback_user_id).is_none() {
            // Not the owner - show alert and return
            bot.answer_callback_query(q.id.clone())
                .text("Bạn không phải người yêu cầu!")
                .show_alert(true)
                .await?;
            return Ok(());
        }
    }

    // Acknowledge callback
    bot.answer_callback_query(q.id.clone()).await?;

    // Get the original message that this message replied to (if any)
    // This is used to reply to the original user request when sending completion messages
    let reply_to_msg_id: Option<MessageId> = q.message.as_ref()
        .and_then(|msg| msg.regular_message())
        .and_then(|msg| msg.reply_to_message())
        .map(|reply| reply.id);

    // User ID for passing to handlers (the owner of the interaction)
    let user_id = callback_user_id;

    match parts.as_slice() {
        ["series", series_id, _user] => {
            handle_series_selected(&bot, chat_id, msg_id, series_id, user_id, &state).await?;
        }
        ["season", series_id, season_id, _user] => {
            handle_season_selected(&bot, chat_id, msg_id, series_id, season_id, user_id, &state).await?;
        }
        ["episode", episode_id, _user] => {
            handle_episode_selected(&bot, chat_id, msg_id, episode_id, user_id, &state).await?;
        }
        ["download", episode_id, _user] => {
            handle_download_start(&bot, chat_id, msg_id, reply_to_msg_id, episode_id, user_id, state.clone(), false).await?;
        }
        ["pixeldrain", episode_id, _user] => {
            handle_download_start(&bot, chat_id, msg_id, reply_to_msg_id, episode_id, user_id, state.clone(), true).await?;
        }
        ["send_cache", episode_id, _user] => {
            handle_send_cache(&bot, chat_id, msg_id, episode_id, user_id, state.clone()).await?;
        }
        ["as", episode_id, idx, _user] => {
            let idx: usize = idx.parse().unwrap_or(0);
            handle_audio_select(&bot, chat_id, msg_id, episode_id, idx, user_id, &state).await?;
        }
        ["ad", episode_id, _user] => {
            handle_audio_download_confirm(&bot, chat_id, msg_id, reply_to_msg_id, episode_id, user_id, state.clone()).await?;
        }
        ["page", season_id, series_id, page, _user] => {
            let page: usize = page.parse().unwrap_or(0);
            handle_page_change(&bot, chat_id, msg_id, season_id, series_id, page, user_id, &state).await?;
        }
        ["cancel", task_id, episode_id, _user] => {
            handle_cancel_download(&bot, chat_id, msg_id, task_id, episode_id, state.clone()).await?;
        }
        ["back", target, extra, _user] => {
            handle_back_navigation(&bot, chat_id, msg_id, target, Some(extra), user_id, &state).await?;
        }
        ["back", target, _user] => {
            handle_back_navigation(&bot, chat_id, msg_id, target, None, user_id, &state).await?;
        }
        ["noop"] => {}
        _ => {
            tracing::warn!("Unknown callback data: {}", data);
        }
    }

    Ok(())
}

async fn handle_series_selected(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    series_id: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    bot.edit_message_text(chat_id, msg_id, "⏳ Đang tải danh sách các mùa...")
        .await?;

    // Get series info for the title
    let series_title = match state.cr_client.get_series(series_id).await {
        Ok(series) => series.title,
        Err(_) => "Unknown".to_string(),
    };

    match state.cr_client.get_seasons(series_id).await {
        Ok(seasons) => {
            if seasons.is_empty() {
                bot.edit_message_text(chat_id, msg_id, "⚠️ Không tìm thấy mùa nào cho series này.")
                    .await?;
                return Ok(());
            }

            // Cache series title for later use
            state.cache_series_title(series_id.to_string(), series_title.clone()).await;

            let keyboard = seasons_keyboard(&seasons, series_id, user_id);
            bot.edit_message_text(
                chat_id,
                msg_id,
                format!("📺 {}\n\n📂 Vui lòng chọn một mùa phim:", series_title),
            )
            .reply_markup(keyboard)
            .await?;
        }
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("❌ Lỗi khi tải các mùa: {}", e))
                .await?;
        }
    }

    Ok(())
}

async fn handle_season_selected(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    series_id: &str,
    season_id: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    bot.edit_message_text(chat_id, msg_id, "⏳ Đang tải danh sách tập phim...")
        .await?;

    // Get series title from cache
    let series_title = state.get_cached_series_title(series_id).await
        .unwrap_or_else(|| "Unknown".to_string());

    match state.cr_client.get_episodes(season_id).await {
        Ok(episodes) => {
            if episodes.is_empty() {
                bot.edit_message_text(chat_id, msg_id, "⚠️ Không tìm thấy tập nào cho mùa này.")
                    .await?;
                return Ok(());
            }

            // Get season title from first episode
            let season_title = episodes.first()
                .and_then(|ep| ep.season_title.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            // Cache season title for later use
            state.cache_season_title(season_id.to_string(), season_title.clone()).await;

            let keyboard = episodes_keyboard(&episodes, season_id, series_id, 0, 8, user_id);
            bot.edit_message_text(
                chat_id,
                msg_id,
                format!(
                    "📺 {}\n📁 {}\n\n🎬 Chọn một tập ({} có sẵn):",
                    series_title, season_title, episodes.len()
                ),
            )
            .reply_markup(keyboard)
            .await?;

            state.cache_episodes(season_id.to_string(), episodes).await;
        }
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("❌ Lỗi khi tải danh sách tập phim: {}", e))
                .await?;
        }
    }

    Ok(())
}

async fn handle_page_change(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    season_id: &str,
    series_id: &str,
    page: usize,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    if let Some(episodes) = state.get_cached_episodes(season_id).await {
        let keyboard = episodes_keyboard(&episodes, season_id, series_id, page, 8, user_id);
        bot.edit_message_reply_markup(chat_id, msg_id)
            .reply_markup(keyboard)
            .await?;
    }
    Ok(())
}

async fn handle_episode_selected(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    episode_id: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    bot.edit_message_text(chat_id, msg_id, "⏳ Đang tải thông tin tập phim...")
        .await?;

    let cached_file_opt = if let Some(ref db) = state.database {
        db.get_cached_file(episode_id).await.unwrap_or(None)
    } else {
        None
    };

    let pixeldrain_enabled = state.config.download.pixeldrain_api_key.is_some();

    match state.cr_client.get_episode(episode_id).await {
        Ok(episode) => {
            let series_title = episode.series_title.as_deref().unwrap_or("Không rõ");
            let season_title = episode.season_title.as_deref().unwrap_or("Không rõ");

            let mut info = format!(
                "📺 {} | 📁 {}\n\n\
                🎬 {}\n\n\
                🔢 Tập: {}\n\
                ⏱ Thời lượng: {}\n\
                🔊 Âm thanh: {}\n\n\
                📝 {}",
                series_title,
                season_title,
                episode.title,
                episode.display_number(),
                episode.duration_formatted(),
                episode.audio_locale.as_deref().unwrap_or("Không rõ"),
                episode.description.as_deref().unwrap_or("Không có mô tả"),
            );

            let keyboard = if cached_file_opt.is_some() {
                info.push_str("\n\n✅ <b>File có sẵn trong Cache!</b>");
                let mut buttons = Vec::new();
                buttons.push(vec![InlineKeyboardButton::callback("🚀 Gửi ngay từ Cache", format!("send_cache:{}:{}", episode_id, user_id))]);

                let mut back_row = Vec::new();
                if let Some(season_id) = &episode.season_id {
                    back_row.push(InlineKeyboardButton::callback("⬅️ Quay lại", format!("back:season:{}:{}", season_id, user_id)));
                } else {
                    back_row.push(InlineKeyboardButton::callback("⬅️ Quay lại", format!("back:search:{}", user_id)));
                }
                buttons.push(back_row);
                InlineKeyboardMarkup::new(buttons)
            } else {
                episode_actions_keyboard_with_pixeldrain(
                    episode_id,
                    episode.season_id.as_deref().unwrap_or(""),
                    pixeldrain_enabled,
                    user_id,
                )
            };

            bot.edit_message_text(chat_id, msg_id, info)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(keyboard)
                .await?;
        }
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("❌ Lỗi khi tải thông tin tập phim: {}", e))
                .await?;
        }
    }

    Ok(())
}

async fn handle_send_cache(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    episode_id: &str,
    user_id: i64,
    state: Arc<BotState>,
) -> ResponseResult<()> {
    if let Some(ref db) = state.database {
        match db.get_cached_file(episode_id).await {
            Ok(Some(cached)) => {
                tracing::info!("Processing send_cache for episode {}", episode_id);

                // Check subtitle freshness before serving cache
                if !check_subtitle_freshness(&state, episode_id, cached.subtitle_locales.as_ref()).await {
                    tracing::info!("Subtitles changed for {}, invalidating cache and re-downloading", episode_id);
                    invalidate_all_caches(&state, episode_id).await;
                    bot.edit_message_text(chat_id, msg_id, "🔄 Phụ đề đã thay đổi. Đang tải lại...").await?;
                    handle_download_start(bot, chat_id, msg_id, None, episode_id, user_id, state, false).await?;
                    return Ok(());
                }

                bot.edit_message_text(chat_id, msg_id, "✅ Tìm thấy trong cache! 📤 Đang gửi...").await?;

                match forward_cached_file(
                    bot,
                    chat_id,
                    &cached.file_id,
                    &cached.filename,
                    cached.file_size,
                    Some(msg_id),
                    cached.storage_chat_id,
                    cached.message_id,
                    cached.audio_locale.as_deref(),
                    cached.subtitle_locales.as_deref(),
                ).await {
                    Ok(_) => {
                        let _ = db.increment_forward_count(episode_id).await;

                        let message = build_cache_hit_message(
                            &cached.filename,
                            bytes_to_mb(cached.file_size),
                            audio_or_default(cached.audio_locale.as_deref()),
                            &format_optional_subtitles(cached.subtitle_locales.as_ref()),
                            None, None
                        );

                        let keyboard = download_complete_keyboard(episode_id, user_id);
                        bot.edit_message_text(chat_id, msg_id, message)
                            .reply_markup(keyboard)
                            .await?;
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!("Failed to forward cached file: {}, triggering download...", e);

                        handle_download_start(bot, chat_id, msg_id, None, episode_id, user_id, state, false).await?;
                    }
                }
            }
            Ok(None) => {
                bot.edit_message_text(chat_id, msg_id, "⚠️ Không tìm thấy trong cache. ⬇️ Bắt đầu tải xuống...").await?;
                handle_download_start(bot, chat_id, msg_id, None, episode_id, user_id, state, false).await?;
            }
            Err(e) => {
                bot.edit_message_text(chat_id, msg_id, format!("❌ Lỗi cơ sở dữ liệu: {}", e)).await?;
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
enum DownloadResultMsg {
    Buzzheavier { filename: String, size: u64, download_link: String },
    Pixeldrain { filename: String, size: u64, download_link: String },
    Telegram { filename: String },
    Failed { error: String },
}

async fn notify_subscribers_and_cleanup(
    bot: &Bot,
    bot_state: &Arc<BotState>,
    episode_id: &str,
    result: DownloadResultMsg,
) {
    if let Some(ref db) = bot_state.database {
        if let Err(e) = db.remove_active_download(episode_id).await {
            tracing::warn!("Failed to remove active download record: {}", e);
        }
    }

    let subscribers = bot_state.take_download_subscribers(episode_id).await;
    if subscribers.is_empty() {
        return;
    }

    for subscriber in subscribers {
        let message = match &result {
            DownloadResultMsg::Buzzheavier { filename, size, download_link } => {
                build_subscriber_notification(filename, *size, Some("Buzzheavier"), Some(download_link))
            }
            DownloadResultMsg::Pixeldrain { filename, size, download_link } => {
                build_subscriber_notification(filename, *size, Some("Pixeldrain"), Some(download_link))
            }
            DownloadResultMsg::Telegram { filename } => {
                build_subscriber_notification(filename, 0, None, None)
            }
            DownloadResultMsg::Failed { error } => {
                build_error_message(error)
            }
        };

        let keyboard = download_complete_keyboard(episode_id, subscriber.user_id);
        let _ = bot.edit_message_text(subscriber.chat_id, subscriber.message_id, &message)
            .reply_markup(keyboard)
            .await;
    }
}

/// Handle audio language selection toggle
async fn handle_audio_select(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    episode_id: &str,
    idx: usize,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let key = format!("{}:{}", user_id, episode_id);
    if let Some(updated) = state.toggle_audio_selection(&key, idx).await {
        let keyboard = audio_selection_keyboard(
            &updated.versions,
            &updated.selected_indices,
            episode_id,
            user_id,
        );
        bot.edit_message_reply_markup(chat_id, msg_id)
            .reply_markup(keyboard)
            .await?;
    }
    Ok(())
}

/// Handle audio selection confirm -> start download with selected audio tracks
async fn handle_audio_download_confirm(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    reply_to_msg_id: Option<MessageId>,
    episode_id: &str,
    user_id: i64,
    state: Arc<BotState>,
) -> ResponseResult<()> {
    let key = format!("{}:{}", user_id, episode_id);
    let selection = match state.get_audio_selection(&key).await {
        Some(s) => s,
        None => {
            bot.edit_message_text(chat_id, msg_id, "⚠️ Phiên chọn ngôn ngữ đã hết hạn. Vui lòng thử lại.").await?;
            return Ok(());
        }
    };

    // Clean up selection state
    state.remove_audio_selection(&key).await;

    let playback = &selection.playback;

    if playback.url.is_none() {
        bot.edit_message_text(chat_id, msg_id, "⚠️ Không có URL luồng khả dụng.").await?;
        return Ok(());
    }

    bot.edit_message_text(chat_id, msg_id, "⬇️ Đang chuẩn bị tải xuống...").await?;

    // Determine which version is primary (first selected) and which are additional
    let selected_versions: Vec<&Version> = selection.selected_indices.iter()
        .filter_map(|&i| selection.versions.get(i))
        .collect();

    if selected_versions.is_empty() {
        bot.edit_message_text(chat_id, msg_id, "⚠️ Chưa chọn ngôn ngữ nào.").await?;
        return Ok(());
    }

    // The primary stream uses the already-fetched playback URL
    // Additional versions need their own playback fetch
    let mut additional_audio_versions: Vec<AudioVersionInfo> = Vec::new();

    // Skip the first selected (it uses the primary stream), fetch playback for the rest
    for version in selected_versions.iter().skip(1) {
        let guid = match &version.guid {
            Some(g) => g.clone(),
            None => continue,
        };
        let locale = version.audio_locale.as_deref().unwrap_or("unknown").to_string();

        match state.cr_client.get_playback(&guid).await {
            Ok(version_playback) => {
                if let Some(version_url) = version_playback.url {
                    additional_audio_versions.push(AudioVersionInfo {
                        audio_locale: locale,
                        guid: guid.clone(),
                        stream_url: version_url,
                        drm_pssh: version_playback.drm.as_ref().and_then(|d| d.pssh.clone()),
                        video_token: version_playback.token.clone(),
                        content_id: Some(episode_id.to_string()),
                    });
                }
            }
            Err(e) => {
                tracing::warn!("Failed to fetch playback for audio version {}: {}", locale, e);
            }
        }
    }

    let primary_audio_locale = selected_versions.first()
        .and_then(|v| v.audio_locale.clone())
        .or_else(|| playback.audio_locale.clone());

    // Call handle_download_start with the pre-built additional audio versions
    handle_download_start_with_audio(
        bot, chat_id, msg_id, reply_to_msg_id, episode_id, user_id, state,
        false, additional_audio_versions, primary_audio_locale, true,
    ).await
}

async fn handle_download_start(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    reply_to_msg_id: Option<MessageId>,
    episode_id: &str,
    user_id: i64,
    state: Arc<BotState>,
    use_pixeldrain: bool,
) -> ResponseResult<()> {
    handle_download_start_with_audio(
        bot, chat_id, msg_id, reply_to_msg_id, episode_id, user_id, state,
        use_pixeldrain, Vec::new(), None, false,
    ).await
}

async fn handle_download_start_with_audio(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    reply_to_msg_id: Option<MessageId>,
    episode_id: &str,
    user_id: i64,
    state: Arc<BotState>,
    use_pixeldrain: bool,
    extra_audio_versions: Vec<AudioVersionInfo>,
    extra_primary_locale: Option<String>,
    audio_already_selected: bool,
) -> ResponseResult<()> {
    tracing::info!("handle_download_start: episode_id={}, use_pixeldrain={}, has_database={}, extra_audio={}",
        episode_id, use_pixeldrain, state.database.is_some(), extra_audio_versions.len());

    if use_pixeldrain {
        if let Some(ref db) = state.database {
            // Find first available service cache and its subtitle locales for freshness check
            let service_cache = 'service_cache: {
                if let Ok(Some(cached)) = db.get_cached_buzzheavier(episode_id).await {
                    break 'service_cache Some(("buzzheavier", cached.filename.clone(), cached.file_size,
                        cached.audio_locale.clone(), cached.subtitle_locales.clone(),
                        cached.download_url.clone()));
                }
                if let Ok(Some(cached)) = db.get_cached_pixeldrain(episode_id).await {
                    break 'service_cache Some(("pixeldrain", cached.filename.clone(), cached.file_size,
                        cached.audio_locale.clone(), cached.subtitle_locales.clone(),
                        cached.download_url.clone()));
                }
                if let Ok(Some(cached)) = db.get_cached_gofile(episode_id).await {
                    break 'service_cache Some(("gofile", cached.filename.clone(), cached.file_size,
                        cached.audio_locale.clone(), cached.subtitle_locales.clone(),
                        cached.download_url.clone()));
                }
                None
            };

            if let Some((service, filename, file_size, audio_locale, subtitle_locales, download_url)) = service_cache {
                // Check subtitle freshness once
                if check_subtitle_freshness(&state, episode_id, subtitle_locales.as_ref()).await {
                    // Fresh - serve from cache
                    match service {
                        "buzzheavier" => { let _ = db.increment_buzzheavier_serve_count(episode_id).await; }
                        "pixeldrain" => { let _ = db.increment_pixeldrain_serve_count(episode_id).await; }
                        "gofile" => { let _ = db.increment_gofile_serve_count(episode_id).await; }
                        _ => {}
                    }
                    let service_name = match service {
                        "buzzheavier" => "Buzzheavier",
                        "pixeldrain" => "Pixeldrain",
                        "gofile" => "Gofile",
                        _ => service,
                    };
                    let message = build_cache_hit_message(
                        &filename, bytes_to_mb(file_size),
                        audio_or_default(audio_locale.as_deref()),
                        &format_optional_subtitles(subtitle_locales.as_ref()),
                        Some(service_name), Some(&download_url)
                    );
                    let keyboard = download_complete_keyboard(episode_id, user_id);
                    bot.edit_message_text(chat_id, msg_id, message).reply_markup(keyboard).await?;
                    return Ok(());
                } else {
                    // Stale - invalidate all caches
                    tracing::info!("Subtitles changed for {}, invalidating all caches", episode_id);
                    invalidate_all_caches(&state, episode_id).await;
                }
            }
        }
    }

    // Logic kiểm tra Cache Telegram
    if !use_pixeldrain {
        tracing::info!("Checking Telegram cache for episode {}", episode_id);
        if let Some(ref db) = state.database {
            match db.get_cached_file(episode_id).await {
                Ok(Some(cached)) => {
                    tracing::info!("Found Telegram cache for episode {}: file_id={}", episode_id, cached.file_id);

                    // Check subtitle freshness before serving
                    if !check_subtitle_freshness(&state, episode_id, cached.subtitle_locales.as_ref()).await {
                        tracing::info!("Subtitles changed for {}, invalidating cache and re-downloading", episode_id);
                        invalidate_all_caches(&state, episode_id).await;
                        // Fall through to download below
                    } else {
                        bot.edit_message_text(chat_id, msg_id, "✅ Tìm thấy trong cache! 📤 Đang gửi...").await?;

                        match forward_cached_file(
                            bot,
                            chat_id,
                            &cached.file_id,
                            &cached.filename,
                            cached.file_size,
                            Some(msg_id),
                            cached.storage_chat_id,
                            cached.message_id,
                            cached.audio_locale.as_deref(),
                            cached.subtitle_locales.as_deref(),
                        ).await {
                            Ok(_) => {
                                let _ = db.increment_forward_count(episode_id).await;

                                let message = build_cache_hit_message(
                                    &cached.filename,
                                    bytes_to_mb(cached.file_size),
                                    audio_or_default(cached.audio_locale.as_deref()),
                                    &format_optional_subtitles(cached.subtitle_locales.as_ref()),
                                    None, None
                                );

                                let keyboard = download_complete_keyboard(episode_id, user_id);
                                bot.edit_message_text(chat_id, msg_id, message)
                                    .reply_markup(keyboard)
                                    .await?;
                                return Ok(());
                            }
                            Err(e) => {
                                tracing::warn!("Failed to forward cached file: {}, will re-download", e);
                            }
                        }
                    }
                }
                Ok(None) => tracing::info!("No Telegram cache found for episode {}", episode_id),
                Err(e) => tracing::warn!("Error checking Telegram cache for {}: {}", episode_id, e),
            }
        } else {
            tracing::warn!("No database configured, skipping cache check");
        }
    }

    if let Some(ref db) = state.database {
        if let Ok(Some(active)) = db.get_active_download(episode_id).await {
            let subscriber = DownloadSubscriber { chat_id, message_id: msg_id, user_id };
            state.add_download_subscriber(episode_id.to_string(), subscriber).await;

            let subscriber_count = state.get_subscriber_count(episode_id).await;
            let status_msg = build_queue_message(&active.title, &active.phase, active.progress, subscriber_count);
            bot.edit_message_text(chat_id, msg_id, status_msg).await?;
            return Ok(());
        }
    }

    bot.edit_message_text(chat_id, msg_id, "⬇️ Bắt đầu tải xuống...").await?;

    let mut episode = match state.cr_client.get_episode(episode_id).await {
        Ok(e) => e,
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("❌ Lỗi: {}", e)).await?;
            return Ok(());
        }
    };

    // Ensure series_title is set - try from API response first, then cached title
    if episode.series_title.is_none() {
        if let Some(ref series_id) = episode.series_id {
            if let Some(cached_title) = state.get_cached_series_title(series_id).await {
                episode.series_title = Some(cached_title);
            }
        }
    }

    // Ensure season_title is set - try from API response first, then cached title
    if episode.season_title.is_none() {
        if let Some(ref season_id) = episode.season_id {
            if let Some(cached_title) = state.get_cached_season_title(season_id).await {
                episode.season_title = Some(cached_title);
            }
        }
    }

    // Debug: log available versions from episode data
    if let Some(ref versions) = episode.versions {
        tracing::info!("Episode {} has {} audio versions: {:?}",
            episode_id, versions.len(),
            versions.iter().map(|v| v.audio_locale.as_deref().unwrap_or("?")).collect::<Vec<_>>()
        );
    } else {
        tracing::info!("Episode {} has no versions field in CMS response", episode_id);
    }

    let playback = match state.cr_client.get_playback(episode_id).await {
        Ok(p) => p,
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("❌ Lỗi khi lấy thông tin luồng: {}", e)).await?;
            return Ok(());
        }
    };

    let stream_url = match playback.url.clone() {
        Some(url) => url,
        None => {
            bot.edit_message_text(chat_id, msg_id, "⚠️ Không có URL luồng khả dụng.").await?;
            return Ok(());
        }
    };

    // Check if multiple audio versions are available - show selection UI
    // Skip if user already went through audio selection
    if !audio_already_selected && extra_audio_versions.is_empty() {
    let versions_source = episode.versions.as_ref().or(playback.versions.as_ref());
    if let Some(versions) = versions_source {
        if versions.len() >= 2 {
            // Pre-select the original version or the first one
            let default_idx = versions.iter().position(|v| v.original == Some(true)).unwrap_or(0);

            let selection_key = format!("{}:{}", user_id, episode_id);
            let selection_state = AudioSelectionState {
                versions: versions.clone(),
                selected_indices: vec![default_idx],
                episode_id: episode_id.to_string(),
                playback: playback.clone(),
                episode: episode.clone(),
            };
            state.set_audio_selection(selection_key, selection_state).await;

            let keyboard = audio_selection_keyboard(versions, &[default_idx], episode_id, user_id);
            bot.edit_message_text(
                chat_id,
                msg_id,
                format!("🔊 Chọn ngôn ngữ âm thanh ({} khả dụng):", versions.len()),
            )
            .reply_markup(keyboard)
            .await?;
            return Ok(());
        }
    }
    }

    let mut all_subtitles: Vec<_> = playback.subtitles.values().cloned().collect();
    all_subtitles.extend(playback.captions.values().cloned());

    let task = DownloadTask {
        id: uuid::Uuid::new_v4().to_string(),
        episode: episode.clone(),
        stream_url,
        drm_pssh: playback.drm.as_ref().and_then(|d| d.pssh.clone()),
        subtitles: all_subtitles,
        video_token: playback.token.clone(),
        content_id: Some(episode_id.to_string()),
        additional_audio_versions: extra_audio_versions,
        primary_audio_locale: extra_primary_locale.or_else(|| playback.audio_locale.clone()),
    };

    let task_id = task.id.clone();
    let progress = new_progress();

    if let Some(ref db) = state.database {
        let mut active_download = ActiveDownload::new(
            episode_id.to_string(),
            episode.title.clone(),
            user_id,
            use_pixeldrain,
        );
        if let Some(ref series_title) = episode.series_title {
            active_download = active_download.with_series_title(series_title.clone());
        }

        match db.create_active_download(&active_download).await {
            Ok(false) => {
                let subscriber = DownloadSubscriber { chat_id, message_id: msg_id, user_id };
                state.add_download_subscriber(episode_id.to_string(), subscriber).await;
                bot.edit_message_text(chat_id, msg_id, "⏳ Tập tin này đang được tải xuống. 📤 Bạn sẽ nhận được nó khi sẵn sàng.").await?;
                return Ok(());
            }
            Err(e) => tracing::warn!("Failed to create active download record: {}", e),
            _ => {}
        }
    }

    let keyboard = download_progress_keyboard(&task_id, episode_id, user_id);
    bot.edit_message_text(chat_id, msg_id, "⬇️ Đang tải xuống... 0%").reply_markup(keyboard).await?;

    let bot_clone = bot.clone();
    let download_manager = state.download_manager.clone();
    let progress_clone = progress.clone();
    let auth_token = state.cr_client.access_token().await.unwrap_or_default();
    let database = state.database.clone();
    let storage_chat_id = state.config.telegram.storage_chat_id;
    let pixeldrain_api_key = state.config.download.pixeldrain_api_key.clone();
    let buzzheavier_account_id = state.config.download.buzzheavier_account_id.clone();
    let buzzheavier_parent_id = state.config.download.buzzheavier_parent_id.clone();
    let gofile_token = state.config.download.gofile_token.clone();
    let upload_preference = state.config.download.upload_preference.clone();
    let preferred_upload_service = state.config.download.preferred_upload_service.clone();
    let episode_id_clone = episode_id.to_string();
    let episode_id_for_keyboard = episode_id.to_string();
    let bot_state = state.clone();
    let user_id_clone = user_id;
    let reply_to_msg_id_clone = reply_to_msg_id;
    let temp_dir_for_cleanup = state.config.download.temp_dir.join(&task_id);

    tokio::spawn(async move {
        let update_task = {
            let bot = bot_clone.clone();
            let progress = progress_clone.clone();
            let task_id = task_id.clone();
            let database = database.clone();
            let episode_id = episode_id_clone.clone();
            let episode_id_kb = episode_id_for_keyboard.clone();

            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    let p = progress.read().await;
                    let status = p.format_progress();

                    if let Some(ref db) = database {
                        let phase = format!("{:?}", p.phase);
                        let progress_pct = if p.total_segments > 0 {
                            ((p.completed_segments as f64 / p.total_segments as f64) * 100.0) as u8
                        } else { 0 };
                        let _ = db.update_active_download_progress(
                            &episode_id, &phase, progress_pct, p.downloaded_bytes,
                            if p.estimated_file_size > 0 { Some(p.estimated_file_size) } else { None },
                            if p.current_speed > 0.0 { Some(p.current_speed as u64) } else { None },
                        ).await;
                    }

                    // Hide cancel button during upload phase (can't cancel uploads)
                    let is_uploading = matches!(p.phase, crate::download::DownloadPhase::Uploading { .. });
                    if is_uploading {
                        let _ = bot.edit_message_text(chat_id, msg_id, &status)
                            .reply_markup(InlineKeyboardMarkup::new(Vec::<Vec<InlineKeyboardButton>>::new())).await;
                    } else {
                        let keyboard = download_progress_keyboard(&task_id, &episode_id_kb, user_id_clone);
                        let _ = bot.edit_message_text(chat_id, msg_id, &status).reply_markup(keyboard).await;
                    }

                    if matches!(p.phase, crate::download::DownloadPhase::Completed | crate::download::DownloadPhase::Failed) {
                        break;
                    }
                }
            })
        };

        let max_retries = 3;
        let mut retry_count = 0;
        let mut video_resume_segment = 0usize;
        let mut audio_resume_segment = 0usize;
        let mut current_task = task.clone();
        let mut current_auth_token = auth_token.clone();

        let download_result = loop {
            let result = download_manager
                .download_with_resume(&current_task, &current_auth_token, progress_clone.clone(), video_resume_segment, audio_resume_segment)
                .await;

            match result {
                Ok(r) => break Ok(r),
                Err(crate::error::Error::StreamUrlExpired { video_segments_completed, audio_segments_completed, .. }) => {
                    retry_count += 1;
                    if retry_count > max_retries {
                        break Err(crate::error::Error::Download("Stream URL expired and max retries exceeded".to_string()));
                    }

                    video_resume_segment = video_segments_completed;
                    audio_resume_segment = audio_segments_completed;

                    match bot_state.cr_client.force_refresh_direct().await {
                        Ok(new_token) => current_auth_token = new_token,
                        Err(e) => break Err(crate::error::Error::Auth(format!("Failed to refresh token: {}", e))),
                    }

                    match bot_state.cr_client.get_playback(&episode_id_clone).await {
                        Ok(new_playback) => {
                            if let Some(new_url) = new_playback.url {
                                current_task = crate::download::DownloadTask {
                                    stream_url: new_url,
                                    video_token: new_playback.token.clone(),
                                    ..current_task
                                };
                            } else {
                                break Err(crate::error::Error::Download("No stream URL in refreshed playback".to_string()));
                            }
                        }
                        Err(e) => break Err(e),
                    }
                    continue;
                }
                Err(e) => break Err(e),
            }
        };

        match download_result {
            Ok(result) => {
                const MAX_TELEGRAM_SIZE_BYTES: u64 = 2000u64 * 1024 * 1024;
                // const MAX_TELEGRAM_SIZE_BYTES: u64 = 1;
                let is_large_file = result.size > MAX_TELEGRAM_SIZE_BYTES;
                let prefer_service = upload_preference == crate::config::UploadPreference::Service;
                // Use service if: file is too large for Telegram OR user prefers service upload
                let should_use_service = is_large_file || prefer_service;

                tracing::info!(
                    "Download finished. Size: {} bytes ({}). Large file: {}. Prefer service: {}. Decision: {}",
                    result.size,
                    format_size(result.size),
                    is_large_file,
                    prefer_service,
                    if should_use_service { "Service" } else { "Telegram" }
                );

                if should_use_service {
                    // Check if credentials are valid (not None and not empty string)
                    let has_buzzheavier = buzzheavier_account_id.as_ref().map(|s| !s.is_empty()).unwrap_or(false);
                    let has_pixeldrain = pixeldrain_api_key.as_ref().map(|s| !s.is_empty()).unwrap_or(false);
                    let has_gofile = gofile_token.as_ref().map(|s| !s.is_empty()).unwrap_or(false);

                    tracing::info!(
                        "Service upload check: has_buzzheavier={}, has_pixeldrain={}, has_gofile={}, preferred={:?}",
                        has_buzzheavier, has_pixeldrain, has_gofile, preferred_upload_service
                    );

                    // If file is too large for Telegram AND no service credentials are available
                    if is_large_file && !has_buzzheavier && !has_pixeldrain && !has_gofile {
                        update_task.abort();
                        let _ = tokio::fs::remove_file(&result.path).await;
                        if result.temp_dir.exists() { let _ = tokio::fs::remove_dir_all(&result.temp_dir).await; }
                        let _ = bot_clone.edit_message_text(chat_id, msg_id, format!(
                            "❌ Không thể tải lên!\n\n\
                            📦 Kích thước: {} (vượt quá giới hạn Telegram 2GB)\n\n\
                            ⚙️ Không có dịch vụ upload nào được cấu hình.\n\
                            Vui lòng cấu hình Buzzheavier, Pixeldrain hoặc Gofile trong config.toml",
                            format_size(result.size)
                        )).await;
                        notify_subscribers_and_cleanup(&bot_clone, &bot_state, &episode_id_clone,
                            DownloadResultMsg::Failed { error: "File quá lớn và không có dịch vụ upload".to_string() }).await;
                        return;
                    }

                    // Build ordered list of services to try based on preference
                    #[derive(Clone, Copy, PartialEq)]
                    enum UploadService { Buzzheavier, Pixeldrain, Gofile }

                    let mut services_to_try: Vec<UploadService> = Vec::new();

                    // Add preferred service first if available
                    match preferred_upload_service {
                        PreferredUploadService::Buzzheavier => {
                            if has_buzzheavier { services_to_try.push(UploadService::Buzzheavier); }
                            if has_pixeldrain { services_to_try.push(UploadService::Pixeldrain); }
                            if has_gofile { services_to_try.push(UploadService::Gofile); }
                        }
                        PreferredUploadService::Pixeldrain => {
                            if has_pixeldrain { services_to_try.push(UploadService::Pixeldrain); }
                            if has_buzzheavier { services_to_try.push(UploadService::Buzzheavier); }
                            if has_gofile { services_to_try.push(UploadService::Gofile); }
                        }
                        PreferredUploadService::Gofile => {
                            if has_gofile { services_to_try.push(UploadService::Gofile); }
                            if has_buzzheavier { services_to_try.push(UploadService::Buzzheavier); }
                            if has_pixeldrain { services_to_try.push(UploadService::Pixeldrain); }
                        }
                    }

                    let mut errors: Vec<String> = Vec::new();
                    let mut upload_success = false;

                    for (attempt, service) in services_to_try.iter().enumerate() {
                        let service_name = match service {
                            UploadService::Buzzheavier => "Buzzheavier",
                            UploadService::Pixeldrain => "Pixeldrain",
                            UploadService::Gofile => "Gofile",
                        };

                        if attempt > 0 {
                            let _ = bot_clone.edit_message_text(chat_id, msg_id, format!(
                                "⚠️ Tải lên thất bại!\n{}\n\n🔄 Đang chuyển sang {}...",
                                errors.iter().map(|e| format!("• {}", e)).collect::<Vec<_>>().join("\n"),
                                service_name
                            )).await;
                        }

                        let upload_progress = new_upload_progress(result.size, service_name);
                        let upload_update_task = {
                            let bot = bot_clone.clone();
                            let progress = upload_progress.clone();
                            tokio::spawn(async move {
                                loop {
                                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                    let p = progress.read().await;
                                    if p.is_complete { break; }
                                    let status = p.format_message();
                                    let _ = bot.edit_message_text(chat_id, msg_id, &status)
                                        .reply_markup(InlineKeyboardMarkup::new(Vec::<Vec<InlineKeyboardButton>>::new())).await;
                                }
                            })
                        };

                        let upload_result = match service {
                            UploadService::Buzzheavier => {
                                upload_to_buzzheavier(
                                    &result.path, &result.filename,
                                    buzzheavier_account_id.as_ref().unwrap(),
                                    buzzheavier_parent_id.as_deref(),
                                    Some(upload_progress)
                                ).await.map(|r| (r.download_link, r.file_id, r.duration_secs, r.upload_speed_mbps, "Buzzheavier"))
                            }
                            UploadService::Pixeldrain => {
                                upload_to_pixeldrain(
                                    &result.path, &result.filename,
                                    pixeldrain_api_key.as_deref(),
                                    Some(upload_progress)
                                ).await.map(|r| (r.download_link, r.file_id, r.duration_secs, r.upload_speed_mbps, "Pixeldrain"))
                            }
                            UploadService::Gofile => {
                                upload_to_gofile(
                                    &result.path, &result.filename,
                                    gofile_token.as_ref().unwrap(),
                                    Some(upload_progress)
                                ).await.map(|r| (r.download_link, r.file_code, r.duration_secs, r.upload_speed_mbps, "Gofile"))
                            }
                        };

                        upload_update_task.abort();

                        match upload_result {
                            Ok((download_link, file_id, duration_secs, upload_speed_mbps, svc_name)) => {
                                update_task.abort();

                                // Save to cache based on service
                                if let Some(ref db) = database {
                                    let key_pairs: Vec<KeyPair> = result.decryption_keys.iter()
                                        .map(|(kid, key)| KeyPair { kid: kid.clone(), key: key.clone() }).collect();

                                    match service {
                                        UploadService::Buzzheavier => {
                                            let mut cached = CachedBuzzheavierFile::new(
                                                task.episode.id.clone(), file_id.clone(),
                                                download_link.clone(), result.filename.clone(),
                                                result.size, task.episode.title.clone(), key_pairs
                                            );
                                            if let Some(series) = task.episode.series_title.clone() {
                                                cached = cached.with_series_info(series, task.episode.display_number());
                                            }
                                            cached = cached.with_audio_info(
                                                result.audio_locale.clone(),
                                                result.subtitle_locales.clone()
                                            );
                                            cached = cached.with_audio_locales(result.audio_locales.clone());
                                            let _ = db.save_cached_buzzheavier(&cached).await;
                                        }
                                        UploadService::Pixeldrain => {
                                            let mut cached = CachedPixeldrainFile::new(
                                                task.episode.id.clone(), file_id.clone(),
                                                download_link.clone(), result.filename.clone(),
                                                result.size, task.episode.title.clone(), key_pairs
                                            );
                                            if let Some(series) = task.episode.series_title.clone() {
                                                cached = cached.with_series_info(series, task.episode.display_number());
                                            }
                                            cached = cached.with_audio_info(
                                                result.audio_locale.clone(),
                                                result.subtitle_locales.clone()
                                            );
                                            cached = cached.with_audio_locales(result.audio_locales.clone());
                                            let _ = db.save_cached_pixeldrain(&cached).await;
                                        }
                                        UploadService::Gofile => {
                                            let mut cached = CachedGofileFile::new(
                                                task.episode.id.clone(), file_id.clone(),
                                                download_link.clone(), result.filename.clone(),
                                                result.size, task.episode.title.clone(), key_pairs
                                            );
                                            if let Some(series) = task.episode.series_title.clone() {
                                                cached = cached.with_series_info(series, task.episode.display_number());
                                            }
                                            cached = cached.with_audio_info(
                                                result.audio_locale.clone(),
                                                result.subtitle_locales.clone()
                                            );
                                            cached = cached.with_audio_locales(result.audio_locales.clone());
                                            let _ = db.save_cached_gofile(&cached).await;
                                        }
                                    }
                                }

                                let _ = tokio::fs::remove_file(&result.path).await;
                                if result.temp_dir.exists() { let _ = tokio::fs::remove_dir_all(&result.temp_dir).await; }

                                let fallback_note = if attempt > 0 {
                                    format!("✅ Tải lên hoàn tất! (Fallback)\n\n")
                                } else {
                                    String::new()
                                };

                                let message = format!(
                                    "{}{}",
                                    fallback_note,
                                    build_service_completion_message(
                                        &result.filename, result.size, result.audio_locale.as_deref(),
                                        &result.subtitle_locales, duration_secs,
                                        upload_speed_mbps, svc_name, &download_link
                                    )
                                );
                                let keyboard = download_complete_keyboard(&task.episode.id, user_id_clone);
                                send_completion_message(&bot_clone, chat_id, msg_id, reply_to_msg_id_clone, message.clone(), keyboard).await;

                                // Send backup copy to storage channel for large files
                                if let Some(storage_id) = storage_chat_id {
                                    let backup_message = format!(
                                        "📦 *Backup \\- Large File*\n\n\
                                        📺 Series: {}\n\
                                        🎬 Tập: {}\n\
                                        🆔 ID: `{}`\n\n\
                                        {}",
                                        task.episode.series_title.as_deref().unwrap_or("Không rõ")
                                            .replace(".", "\\.").replace("-", "\\-").replace("(", "\\(").replace(")", "\\)"),
                                        task.episode.display_number(),
                                        task.episode.id,
                                        message
                                    );
                                    let _ = bot_clone.send_message(ChatId(storage_id), backup_message)
                                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                        .await;
                                }

                                let result_msg = match service {
                                    UploadService::Buzzheavier => DownloadResultMsg::Buzzheavier {
                                        filename: result.filename.clone(), size: result.size, download_link
                                    },
                                    UploadService::Pixeldrain | UploadService::Gofile => DownloadResultMsg::Pixeldrain {
                                        filename: result.filename.clone(), size: result.size, download_link
                                    },
                                };

                                notify_subscribers_and_cleanup(&bot_clone, &bot_state, &episode_id_clone, result_msg).await;
                                upload_success = true;
                                break;
                            }
                            Err(e) => {
                                tracing::warn!("{} upload failed: {}", service_name, e);
                                errors.push(format!("{}: {}", service_name, e));
                            }
                        }
                    }

                    if !upload_success {
                        update_task.abort();
                        let _ = tokio::fs::remove_file(&result.path).await;
                        if result.temp_dir.exists() { let _ = tokio::fs::remove_dir_all(&result.temp_dir).await; }
                        let _ = bot_clone.edit_message_text(chat_id, msg_id, format!(
                            "❌ Tải lên thất bại!\n\n{}",
                            errors.iter().map(|e| format!("• {}", e)).collect::<Vec<_>>().join("\n")
                        )).await;
                        notify_subscribers_and_cleanup(&bot_clone, &bot_state, &episode_id_clone,
                            DownloadResultMsg::Failed { error: errors.join(", ") }).await;
                    }
                } else {
                    if let (Some(ref db), Some(storage_id)) = (&database, storage_chat_id) {
                        let caption = format!("{}\nTập: {}\nID: {}",
                            task.episode.series_title.as_deref().unwrap_or("Không rõ"),
                            task.episode.display_number(), task.episode.id
                        );

                        match upload_to_storage(
                            &bot_clone, ChatId(storage_id), &result.path, &result.filename,
                            &caption, progress_clone.clone(), result.width, result.height
                        ).await {
                            Ok((file_id, message_id, file_size)) => {
                                let mut cached = CachedFile::new(
                                    task.episode.id.clone(), file_id.clone(), result.filename.clone(),
                                    file_size, task.episode.title.clone(), message_id, storage_id
                                );
                                if let Some(series) = task.episode.series_title.clone() {
                                    cached = cached.with_series_info(
                                        series,
                                        task.episode.season_title.clone(),
                                        task.episode.display_number()
                                    );
                                }
                                cached = cached.with_audio_info(
                                    result.audio_locale.clone(),
                                    result.subtitle_locales.clone()
                                );
                                cached = cached.with_audio_locales(result.audio_locales.clone());
                                let _ = db.save_cached_file(&cached).await;

                                if let Err(e) = forward_cached_file(
                                    &bot_clone,
                                    chat_id,
                                    &file_id,
                                    &result.filename,
                                    file_size,
                                    Some(msg_id),
                                    Some(storage_id),
                                    Some(message_id),
                                    result.audio_locale.as_deref(),
                                    Some(&result.subtitle_locales),
                                ).await {
                                    let _ = bot_clone.edit_message_text(chat_id, msg_id, format!("❌ Lỗi khi chuyển tiếp tập tin: {}", e)).await;
                                    notify_subscribers_and_cleanup(&bot_clone, &bot_state, &episode_id_clone, DownloadResultMsg::Failed { error: e.to_string() }).await;
                                    update_task.abort();
                                    return;
                                }

                                update_task.abort();
                                let _ = tokio::fs::remove_file(&result.path).await;
                                if result.temp_dir.exists() { let _ = tokio::fs::remove_dir_all(&result.temp_dir).await; }

                                let message = build_telegram_completion_message(
                                    &result.filename, result.size, result.audio_locale.as_deref(), &result.subtitle_locales
                                );
                                let keyboard = download_complete_keyboard(&task.episode.id, user_id_clone);
                                send_completion_message(&bot_clone, chat_id, msg_id, reply_to_msg_id_clone, message, keyboard).await;

                                notify_subscribers_and_cleanup(
                                    &bot_clone, &bot_state, &episode_id_clone,
                                    DownloadResultMsg::Telegram { filename: result.filename.clone() }
                                ).await;
                            }
                            Err(e) => {
                                update_task.abort();
                                let _ = tokio::fs::remove_file(&result.path).await;
                                if result.temp_dir.exists() { let _ = tokio::fs::remove_dir_all(&result.temp_dir).await; }
                                let _ = bot_clone.edit_message_text(chat_id, msg_id, format!("❌ Tải lên lưu trữ thất bại: {}", e)).await;
                                notify_subscribers_and_cleanup(&bot_clone, &bot_state, &episode_id_clone, DownloadResultMsg::Failed { error: e.to_string() }).await;
                            }
                        }
                    } else {
                        let upload_result = upload_or_link(&bot_clone, chat_id, &result.path, &result.filename, progress_clone.clone(), result.width, result.height).await;
                        update_task.abort();
                        let _ = tokio::fs::remove_file(&result.path).await;
                        if result.temp_dir.exists() { let _ = tokio::fs::remove_dir_all(&result.temp_dir).await; }

                        match upload_result {
                            Ok(_) => {
                                let message = build_telegram_completion_message(&result.filename, result.size, result.audio_locale.as_deref(), &result.subtitle_locales);
                                let keyboard = download_complete_keyboard(&task.episode.id, user_id_clone);
                                send_completion_message(&bot_clone, chat_id, msg_id, reply_to_msg_id_clone, message, keyboard).await;
                                notify_subscribers_and_cleanup(&bot_clone, &bot_state, &episode_id_clone, DownloadResultMsg::Telegram { filename: result.filename.clone() }).await;
                            }
                            Err(e) => {
                                let _ = bot_clone.edit_message_text(chat_id, msg_id, format!("❌ Tải lên thất bại: {}", e)).await;
                                notify_subscribers_and_cleanup(&bot_clone, &bot_state, &episode_id_clone, DownloadResultMsg::Failed { error: e.to_string() }).await;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                update_task.abort();
                // Cleanup temp directory on download failure
                if temp_dir_for_cleanup.exists() {
                    let _ = tokio::fs::remove_dir_all(&temp_dir_for_cleanup).await;
                }
                let _ = bot_clone.edit_message_text(chat_id, msg_id, format!("❌ Tải xuống thất bại: {}", e)).await;
                notify_subscribers_and_cleanup(&bot_clone, &bot_state, &episode_id_clone, DownloadResultMsg::Failed { error: e.to_string() }).await;
            }
        }
    });

    Ok(())
}

async fn handle_cancel_download(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    task_id: &str,
    episode_id: &str,
    state: Arc<BotState>,
) -> ResponseResult<()> {
    state.download_manager.cancel();
    let temp_dir = state.config.download.temp_dir.join(task_id);
    if temp_dir.exists() { let _ = tokio::fs::remove_dir_all(&temp_dir).await; }

    // Remove active download record from database
    if let Some(ref db) = state.database {
        let _ = db.remove_active_download(episode_id).await;
    }

    // Notify all subscribers that the download was cancelled
    let subscribers = state.take_download_subscribers(episode_id).await;
    for subscriber in subscribers {
        let _ = bot.edit_message_text(
            subscriber.chat_id,
            subscriber.message_id,
            "❌ Tải xuống đã bị huỷ bởi người yêu cầu ban đầu.\n\n💡 Vui lòng tự tải xuống.",
        ).await;
    }

    bot.edit_message_text(chat_id, msg_id, "❌ Tải xuống đã huỷ. Đã dọn dẹp các tệp tạm thời.").await?;
    Ok(())
}

async fn handle_back_navigation(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: MessageId,
    target: &str,
    extra: Option<&str>,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    match target {
        // "search" => {
        //     bot.edit_message_text(chat_id, msg_id, "🔍 Sử dụng /search <tên> hoặc /get <id>").await?;
        // }
        "search" => {
            bot.edit_message_text(chat_id, msg_id, "🔍 Sử dụng /search <tên>").await?;
        }
        "season" => {
            if let Some(season_id) = extra {
                if !season_id.is_empty() {
                    bot.edit_message_text(chat_id, msg_id, "⏳ Đang tải các tập...").await?;
                    match state.cr_client.get_episodes(season_id).await {
                        Ok(episodes) => {
                            if episodes.is_empty() {
                                bot.edit_message_text(chat_id, msg_id, "⚠️ Không tìm thấy tập nào cho mùa này.").await?;
                                return Ok(());
                            }

                            // Get series and season title from first episode
                            let series_title = episodes.first()
                                .and_then(|ep| ep.series_title.clone())
                                .unwrap_or_else(|| "Không rõ".to_string());
                            let season_title = episodes.first()
                                .and_then(|ep| ep.season_title.clone())
                                .unwrap_or_else(|| "Không rõ".to_string());

                            let series_id = episodes.first().and_then(|e| e.series_id.as_deref()).unwrap_or("");
                            let keyboard = episodes_keyboard(&episodes, season_id, series_id, 0, 8, user_id);
                            bot.edit_message_text(
                                chat_id,
                                msg_id,
                                format!(
                                    "📺 {}\n📁 {}\n\n🎬 Chọn một tập ({} có sẵn):",
                                    series_title, season_title, episodes.len()
                                ),
                            )
                            .reply_markup(keyboard)
                            .await?;
                            state.cache_episodes(season_id.to_string(), episodes).await;
                        }
                        Err(e) => {
                            bot.edit_message_text(chat_id, msg_id, format!("❌ Lỗi khi tải các tập: {}", e)).await?;
                        }
                    }
                    return Ok(());
                }
            }
            bot.edit_message_text(chat_id, msg_id, "🔍 Sử dụng /search để bắt đầu tìm kiếm mới.").await?;
        }
        "series" => {
            if let Some(series_id) = extra {
                if !series_id.is_empty() {
                    handle_series_selected(bot, chat_id, msg_id, series_id, user_id, state).await?;
                    return Ok(());
                }
            }
            bot.edit_message_text(chat_id, msg_id, "🔍 Sử dụng /search để bắt đầu tìm kiếm mới.").await?;
        }
        _ => {
            bot.edit_message_text(chat_id, msg_id, "🔍 Sử dụng /search để bắt đầu tìm kiếm mới.").await?;
        }
    }
    Ok(())
}