use crate::config::PreferredUploadService;
use crate::database::models::{ActiveDownload, CachedBuzzheavierFile, CachedGofileFile, CachedFile, CachedPixeldrainFile, KeyPair};
use crate::download::progress::new_progress;
use crate::download::DownloadTask;
use crate::telegram::bot::{BotState, DownloadSubscriber};
use crate::telegram::buzzheavier::upload_to_buzzheavier;
use crate::telegram::commands::build_callback_not_authorized;
use crate::telegram::gofile::upload_to_gofile;
use crate::crunchyroll::types::AudioVersionInfo;
use crate::telegram::keyboards::{
    download_complete_keyboard, download_progress_keyboard,
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

    let s = state.strings;

    // Check if user is allowed to use the bot in this chat
    if !state.is_allowed(callback_user_id, chat_id.0).await {
        let not_authorized_msg = build_callback_not_authorized(&state.config.telegram.owner_users, s);
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
            bot.answer_callback_query(q.id.clone())
                .text(s.not_callback_owner)
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
    let s = state.strings;
    bot.edit_message_text(chat_id, msg_id, s.loading_seasons).await?;

    let series_title = match state.cr_client.get_series(series_id).await {
        Ok(series) => series.title,
        Err(_) => "Unknown".to_string(),
    };

    match state.cr_client.get_seasons(series_id).await {
        Ok(seasons) => {
            if seasons.is_empty() {
                bot.edit_message_text(chat_id, msg_id, s.no_seasons).await?;
                return Ok(());
            }

            state.cache_series_title(series_id.to_string(), series_title.clone()).await;

            let keyboard = seasons_keyboard(&seasons, series_id, user_id, s);
            bot.edit_message_text(
                chat_id,
                msg_id,
                format!("📺 {}\n\n{}", series_title, s.seasons_select),
            )
            .reply_markup(keyboard)
            .await?;
        }
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.seasons_error, e)).await?;
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
    let s = state.strings;
    bot.edit_message_text(chat_id, msg_id, s.loading_episodes).await?;

    let series_title = state.get_cached_series_title(series_id).await
        .unwrap_or_else(|| "Unknown".to_string());

    match state.cr_client.get_episodes(season_id).await {
        Ok(episodes) => {
            if episodes.is_empty() {
                bot.edit_message_text(chat_id, msg_id, s.no_episodes_season).await?;
                return Ok(());
            }

            let season_title = episodes.first()
                .and_then(|ep| ep.season_title.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            state.cache_season_title(season_id.to_string(), season_title.clone()).await;

            let keyboard = episodes_keyboard(&episodes, season_id, series_id, 0, 8, user_id, s);
            bot.edit_message_text(
                chat_id,
                msg_id,
                format!(
                    "📺 {}\n📁 {}\n\n{} ({}):",
                    series_title, season_title, s.episodes_select, episodes.len()
                ),
            )
            .reply_markup(keyboard)
            .await?;

            state.cache_episodes(season_id.to_string(), episodes).await;
        }
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.episodes_error, e)).await?;
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
        let s = state.strings;
        let keyboard = episodes_keyboard(&episodes, season_id, series_id, page, 8, user_id, s);
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
    let s = state.strings;
    bot.edit_message_text(chat_id, msg_id, s.loading_episode_info).await?;

    let cached_file_opt = if let Some(ref db) = state.database {
        db.get_cached_file(episode_id).await.unwrap_or(None)
    } else {
        None
    };

    let pixeldrain_enabled = state.config.download.pixeldrain_api_key.is_some();

    match state.cr_client.get_episode(episode_id).await {
        Ok(episode) => {
            let series_title = episode.series_title.as_deref().unwrap_or(s.unknown_field);
            let season_title = episode.season_title.as_deref().unwrap_or(s.unknown_field);

            let mut info = format!(
                "📺 {} | 📁 {}\n\n\
                🎬 {}\n\n\
                {}: {}\n\
                {}: {}\n\
                {}: {}\n\n\
                📝 {}",
                series_title,
                season_title,
                episode.title,
                s.field_episode, episode.display_number(),
                s.field_duration, episode.duration_formatted(),
                s.field_audio, episode.audio_locale.as_deref().unwrap_or(s.unknown_field),
                episode.description.as_deref().unwrap_or(s.no_description),
            );

            let keyboard = if cached_file_opt.is_some() {
                info.push_str(&format!("\n\n{}", s.cache_available));
                let mut buttons = Vec::new();
                buttons.push(vec![InlineKeyboardButton::callback(s.kb_send_from_cache, format!("send_cache:{}:{}", episode_id, user_id))]);

                let mut back_row = Vec::new();
                if let Some(season_id) = &episode.season_id {
                    back_row.push(InlineKeyboardButton::callback(s.kb_back, format!("back:season:{}:{}", season_id, user_id)));
                } else {
                    back_row.push(InlineKeyboardButton::callback(s.kb_back, format!("back:search:{}", user_id)));
                }
                buttons.push(back_row);
                InlineKeyboardMarkup::new(buttons)
            } else {
                episode_actions_keyboard_with_pixeldrain(
                    episode_id,
                    episode.season_id.as_deref().unwrap_or(""),
                    pixeldrain_enabled,
                    user_id,
                    s,
                )
            };

            bot.edit_message_text(chat_id, msg_id, info)
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_markup(keyboard)
                .await?;
        }
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.episode_info_error, e)).await?;
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

                let s = state.strings;
                // Check subtitle freshness before serving cache
                if !check_subtitle_freshness(&state, episode_id, cached.subtitle_locales.as_ref()).await {
                    tracing::info!("Subtitles changed for {}, invalidating cache and re-downloading", episode_id);
                    invalidate_all_caches(&state, episode_id).await;
                    bot.edit_message_text(chat_id, msg_id, s.cache_invalidated).await?;
                    handle_download_start(bot, chat_id, msg_id, None, episode_id, user_id, state, false).await?;
                    return Ok(());
                }

                bot.edit_message_text(chat_id, msg_id, s.cache_hit_sending).await?;

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
                let s = state.strings;
                bot.edit_message_text(chat_id, msg_id, s.cache_miss_downloading).await?;
                handle_download_start(bot, chat_id, msg_id, None, episode_id, user_id, state, false).await?;
            }
            Err(e) => {
                bot.edit_message_text(chat_id, msg_id, format!("❌ DB: {}", e)).await?;
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
        use_pixeldrain, Vec::new(), None, Vec::new(),
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
    extra_subtitles: Vec<SubtitleTrack>,
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
                        let s_ref = state.strings;
                        bot.edit_message_text(chat_id, msg_id, s_ref.cache_hit_sending).await?;

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

    let s = state.strings;
    bot.edit_message_text(chat_id, msg_id, s.download_starting).await?;

    let mut episode = match state.cr_client.get_episode(episode_id).await {
        Ok(e) => e,
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.download_error, e)).await?;
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
            bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.stream_fetch_error, e)).await?;
            return Ok(());
        }
    };

    let stream_url = match playback.url.clone() {
        Some(url) => url,
        None => {
            bot.edit_message_text(chat_id, msg_id, s.stream_url_unavailable).await?;
            return Ok(());
        }
    };

    // Auto-select audio versions based on preferred_audio config
    let (final_stream_url, final_primary_locale, final_extra_audio, final_extra_subtitles) =
        if extra_audio_versions.is_empty() {
            let versions_source = episode.versions.as_ref().or(playback.versions.as_ref());
            let mut resolved_url = stream_url.clone();
            let mut resolved_locale = extra_primary_locale.clone().or_else(|| playback.audio_locale.clone());
            let mut resolved_extra: Vec<AudioVersionInfo> = Vec::new();
            let mut resolved_subs: Vec<SubtitleTrack> = extra_subtitles.clone();

            if let Some(raw_versions) = versions_source {
                if raw_versions.len() >= 2 {
                    let preferred = &state.config.crunchyroll.preferred_audio;

                    // Pick primary: first preferred locale present, else original, else first
                    let primary = preferred.iter()
                        .find_map(|p| raw_versions.iter().find(|v| v.audio_locale.as_deref() == Some(p.as_str())))
                        .or_else(|| raw_versions.iter().find(|v| v.original == Some(true)))
                        .or_else(|| raw_versions.first());

                    if let Some(prim) = primary {
                        let prim_locale = prim.audio_locale.clone();
                        let prim_guid = prim.guid.clone();

                        // Swap stream if primary differs from what playback returned
                        if prim_locale.as_deref() != playback.audio_locale.as_deref() {
                            if let Some(guid) = prim_guid {
                                if let Ok(prim_pb) = state.cr_client.get_playback(&guid).await {
                                    if let Some(url) = prim_pb.url {
                                        resolved_url = url;
                                        resolved_locale = prim_locale.clone();
                                        // Merge subtitles from primary playback
                                        for sub in prim_pb.subtitles.values().chain(prim_pb.captions.values()) {
                                            if !resolved_subs.iter().any(|s| s.locale == sub.locale) {
                                                resolved_subs.push(sub.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Collect additional preferred locales (skip primary)
                        for pref in preferred.iter() {
                            if pref.as_str() == prim_locale.as_deref().unwrap_or("") { continue; }
                            if let Some(ver) = raw_versions.iter().find(|v| v.audio_locale.as_deref() == Some(pref.as_str())) {
                                if let Some(ref g) = ver.guid {
                                    if let Ok(vp) = state.cr_client.get_playback(g).await {
                                        if let Some(url) = vp.url {
                                            for sub in vp.subtitles.values().chain(vp.captions.values()) {
                                                if !resolved_subs.iter().any(|s| s.locale == sub.locale) {
                                                    resolved_subs.push(sub.clone());
                                                }
                                            }
                                            resolved_extra.push(AudioVersionInfo {
                                                audio_locale: pref.clone(),
                                                guid: g.clone(),
                                                stream_url: url,
                                                drm_pssh: vp.drm.as_ref().and_then(|d| d.pssh.clone()),
                                                video_token: vp.token.clone(),
                                                content_id: Some(episode_id.to_string()),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            (resolved_url, resolved_locale, resolved_extra, resolved_subs)
        } else {
            (stream_url, extra_primary_locale.or_else(|| playback.audio_locale.clone()), extra_audio_versions, extra_subtitles)
        };

    let mut all_subtitles: Vec<_> = playback.subtitles.values().cloned().collect();
    all_subtitles.extend(playback.captions.values().cloned());
    for sub in final_extra_subtitles {
        if !all_subtitles.iter().any(|s| s.locale == sub.locale) {
            all_subtitles.push(sub);
        }
    }

    let task = DownloadTask {
        id: uuid::Uuid::new_v4().to_string().replace('-', "")[..16].to_string(),
        episode: episode.clone(),
        stream_url: final_stream_url,
        drm_pssh: playback.drm.as_ref().and_then(|d| d.pssh.clone()),
        subtitles: all_subtitles,
        video_token: playback.token.clone(),
        content_id: Some(episode_id.to_string()),
        additional_audio_versions: final_extra_audio,
        primary_audio_locale: final_primary_locale,
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
                bot.edit_message_text(chat_id, msg_id, s.already_downloading).await?;
                return Ok(());
            }
            Err(e) => tracing::warn!("Failed to create active download record: {}", e),
            _ => {}
        }
    }

    let keyboard = download_progress_keyboard(&task_id, episode_id, user_id, s);
    bot.edit_message_text(chat_id, msg_id, s.download_progress).reply_markup(keyboard).await?;

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
    let strings = s; // &'static Strings — Copy, safe to capture
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
                        let keyboard = download_progress_keyboard(&task_id, &episode_id_kb, user_id_clone, strings);
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
                let max_telegram_size = bot_state.telegram_max_upload_bytes;
                let is_large_file = result.size > max_telegram_size;
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
                            "❌ Upload failed!\n\n📦 Size: {}\n\n{}",
                            format_size(result.size),
                            strings.file_too_large_no_service
                        )).await;
                        notify_subscribers_and_cleanup(&bot_clone, &bot_state, &episode_id_clone,
                            DownloadResultMsg::Failed { error: strings.file_too_large_no_service.to_string() }).await;
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
                                "{}!\n{}\n\n{} {}...",
                                strings.upload_error,
                                errors.iter().map(|e| format!("• {}", e)).collect::<Vec<_>>().join("\n"),
                                strings.upload_switching,
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
                                    strings.upload_fallback_prefix.to_string()
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
                            "{}!\n\n{}",
                            strings.upload_error,
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
                                    let _ = bot_clone.edit_message_text(chat_id, msg_id, format!("{}: {}", strings.forward_error, e)).await;
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
                                let _ = bot_clone.edit_message_text(chat_id, msg_id, format!("{}: {}", strings.upload_storage_error, e)).await;
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
                                let _ = bot_clone.edit_message_text(chat_id, msg_id, format!("{}: {}", strings.upload_error, e)).await;
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
                let _ = bot_clone.edit_message_text(chat_id, msg_id, format!("{}: {}", strings.download_error, e)).await;
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

    let s = state.strings;
    let subscribers = state.take_download_subscribers(episode_id).await;
    for subscriber in subscribers {
        let _ = bot.edit_message_text(
            subscriber.chat_id,
            subscriber.message_id,
            s.download_cancelled_subscriber,
        ).await;
    }

    bot.edit_message_text(chat_id, msg_id, s.download_cancelled).await?;
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
    let s = state.strings;
    match target {
        "search" => {
            bot.edit_message_text(chat_id, msg_id, s.back_search_hint).await?;
        }
        "season" => {
            if let Some(season_id) = extra {
                if !season_id.is_empty() {
                    bot.edit_message_text(chat_id, msg_id, s.loading_episodes).await?;
                    match state.cr_client.get_episodes(season_id).await {
                        Ok(episodes) => {
                            if episodes.is_empty() {
                                bot.edit_message_text(chat_id, msg_id, s.no_episodes_season).await?;
                                return Ok(());
                            }

                            let series_title = episodes.first()
                                .and_then(|ep| ep.series_title.clone())
                                .unwrap_or_else(|| s.unknown_field.to_string());
                            let season_title = episodes.first()
                                .and_then(|ep| ep.season_title.clone())
                                .unwrap_or_else(|| s.unknown_field.to_string());

                            let series_id = episodes.first().and_then(|e| e.series_id.as_deref()).unwrap_or("");
                            let keyboard = episodes_keyboard(&episodes, season_id, series_id, 0, 8, user_id, s);
                            bot.edit_message_text(
                                chat_id,
                                msg_id,
                                format!(
                                    "📺 {}\n📁 {}\n\n{} ({}):",
                                    series_title, season_title, s.episodes_select, episodes.len()
                                ),
                            )
                            .reply_markup(keyboard)
                            .await?;
                            state.cache_episodes(season_id.to_string(), episodes).await;
                        }
                        Err(e) => {
                            bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.episodes_error, e)).await?;
                        }
                    }
                    return Ok(());
                }
            }
            bot.edit_message_text(chat_id, msg_id, s.back_search_hint_full).await?;
        }
        "series" => {
            if let Some(series_id) = extra {
                if !series_id.is_empty() {
                    handle_series_selected(bot, chat_id, msg_id, series_id, user_id, state).await?;
                    return Ok(());
                }
            }
            bot.edit_message_text(chat_id, msg_id, s.back_search_hint_full).await?;
        }
        _ => {
            bot.edit_message_text(chat_id, msg_id, s.back_search_hint_full).await?;
        }
    }
    Ok(())
}