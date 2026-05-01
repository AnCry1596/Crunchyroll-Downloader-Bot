use crate::config::Config;
use crate::crunchyroll::types::{Episode, StreamResponse, SubtitleTrack, Version};
use crate::crunchyroll::CrunchyrollClient;
use crate::database::models::{AdminUser, AuthorizedChat};
use crate::database::Database;
use crate::download::DownloadManager;
use crate::drm::WidevineCdm;
use crate::error::Result;
use crate::i18n::{Lang, Strings};
use crate::telegram::callbacks::handle_callback;
use crate::telegram::commands::{parse_crunchyroll_input, Command, ContentType, build_welcome_message, build_help_message, build_not_authorized_message, build_donate_message};
use crate::telegram::keyboards::{
    episode_actions_keyboard, episode_actions_keyboard_with_pixeldrain, episodes_keyboard, search_results_keyboard, seasons_keyboard,
};

use crate::tools::ToolManager;
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::dispatching::UpdateFilterExt;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ReplyParameters};
use teloxide::utils::command::BotCommands;
use tokio::sync::RwLock;
use url::Url;

/// Subscriber waiting for a download to complete
#[derive(Debug, Clone)]
pub struct DownloadSubscriber {
    pub chat_id: ChatId,
    pub message_id: MessageId,
    pub user_id: i64,
}

/// State for audio language selection UI
#[derive(Debug, Clone)]
pub struct AudioSelectionState {
    pub versions: Vec<Version>,
    pub selected_indices: Vec<usize>,
    pub episode_id: String,
    pub playback: StreamResponse,
    pub episode: Episode,
    /// Subtitles collected from all version playbacks (pre-fetched, de-duped)
    pub all_subtitles: Vec<SubtitleTrack>,
}

/// Shared bot state
pub struct BotState {
    pub cr_client: CrunchyrollClient,
    pub download_manager: Arc<DownloadManager>,
    pub tool_manager: Arc<ToolManager>,
    pub config: Config,
    pub database: Option<Database>,
    episode_cache: RwLock<HashMap<String, Vec<Episode>>>,
    /// Cache for series titles (series_id -> title)
    series_title_cache: RwLock<HashMap<String, String>>,
    /// Cache for season titles (season_id -> title)
    season_title_cache: RwLock<HashMap<String, String>>,
    /// Subscribers waiting for active downloads to complete (content_id -> subscribers)
    download_subscribers: RwLock<HashMap<String, Vec<DownloadSubscriber>>>,
    /// Pending audio language selections per user+episode (key: "user_id:episode_id")
    audio_selections: RwLock<HashMap<String, AudioSelectionState>>,
    /// Maximum file size for direct Telegram upload (bytes)
    /// 50MB if no api_url, 100MB if behind Cloudflare, 2048MB if own server
    pub telegram_max_upload_bytes: u64,
    /// UI language strings
    pub strings: &'static Strings,
    /// Bot display name fetched from Telegram API
    pub bot_name: String,
}

impl BotState {
    pub fn new(
        cr_client: CrunchyrollClient,
        download_manager: DownloadManager,
        tool_manager: ToolManager,
        config: Config,
        database: Option<Database>,
        telegram_max_upload_bytes: u64,
        bot_name: String,
    ) -> Self {
        let lang = Lang::from_str(&config.telegram.language);
        let strings = Strings::get(&lang);
        Self {
            cr_client,
            download_manager: Arc::new(download_manager),
            tool_manager: Arc::new(tool_manager),
            config,
            database,
            episode_cache: RwLock::new(HashMap::new()),
            series_title_cache: RwLock::new(HashMap::new()),
            season_title_cache: RwLock::new(HashMap::new()),
            download_subscribers: RwLock::new(HashMap::new()),
            audio_selections: RwLock::new(HashMap::new()),
            telegram_max_upload_bytes,
            strings,
            bot_name,
        }
    }

    /// Cache episodes for a season
    pub async fn cache_episodes(&self, season_id: String, episodes: Vec<Episode>) {
        let mut cache = self.episode_cache.write().await;
        cache.insert(season_id, episodes);

        // Limit cache size
        if cache.len() > 10 {
            if let Some(key) = cache.keys().next().cloned() {
                cache.remove(&key);
            }
        }
    }

    /// Get cached episodes
    pub async fn get_cached_episodes(&self, season_id: &str) -> Option<Vec<Episode>> {
        let cache = self.episode_cache.read().await;
        cache.get(season_id).cloned()
    }

    /// Cache series title
    pub async fn cache_series_title(&self, series_id: String, title: String) {
        let mut cache = self.series_title_cache.write().await;
        cache.insert(series_id, title);

        // Limit cache size
        if cache.len() > 20 {
            if let Some(key) = cache.keys().next().cloned() {
                cache.remove(&key);
            }
        }
    }

    /// Get cached series title
    pub async fn get_cached_series_title(&self, series_id: &str) -> Option<String> {
        let cache = self.series_title_cache.read().await;
        cache.get(series_id).cloned()
    }

    /// Cache season title
    pub async fn cache_season_title(&self, season_id: String, title: String) {
        let mut cache = self.season_title_cache.write().await;
        cache.insert(season_id, title);

        // Limit cache size
        if cache.len() > 20 {
            if let Some(key) = cache.keys().next().cloned() {
                cache.remove(&key);
            }
        }
    }

    /// Get cached season title
    pub async fn get_cached_season_title(&self, season_id: &str) -> Option<String> {
        let cache = self.season_title_cache.read().await;
        cache.get(season_id).cloned()
    }

    /// Check if user is an owner (defined in config.toml)
    pub fn is_owner(&self, user_id: i64) -> bool {
        self.config.telegram.is_owner(user_id)
    }

    /// Check if user is an admin (from database)
    pub async fn is_admin(&self, user_id: i64) -> bool {
        if let Some(ref db) = self.database {
            db.is_admin(user_id).await.unwrap_or(false)
        } else {
            false
        }
    }

    /// Check if user is owner or admin
    pub async fn is_owner_or_admin(&self, user_id: i64) -> bool {
        self.is_owner(user_id) || self.is_admin(user_id).await
    }

    /// Check if a user is allowed to use the bot in a given chat
    /// Rules:
    /// - If no owners configured: allow everyone (backwards compatible)
    /// - Owners/admins: always allowed in private chat
    /// - Group chats: must be authorized in database
    /// - Private chats for non-owner/admin: must be authorized in database
    pub async fn is_allowed(&self, user_id: i64, chat_id: i64) -> bool {
        // If no owners configured, allow everyone (backwards compatible)
        if !self.config.telegram.has_owners() {
            return true;
        }

        // Owners always allowed
        if self.is_owner(user_id) {
            return true;
        }

        // Admins always allowed in private chat
        let is_private = chat_id > 0;
        if is_private && self.is_admin(user_id).await {
            return true;
        }

        // Check if chat is authorized in database
        if let Some(ref db) = self.database {
            // For group chats, check if the group is authorized
            if !is_private {
                return db.is_chat_authorized(chat_id).await.unwrap_or(false);
            }
            // For private chats, check if the user's private chat is authorized
            return db.is_chat_authorized(chat_id).await.unwrap_or(false);
        }

        false
    }

    /// Add a subscriber waiting for a download
    pub async fn add_download_subscriber(&self, content_id: String, subscriber: DownloadSubscriber) {
        let mut subscribers = self.download_subscribers.write().await;
        subscribers
            .entry(content_id)
            .or_insert_with(Vec::new)
            .push(subscriber);
    }

    /// Get and remove all subscribers for a completed download
    pub async fn take_download_subscribers(&self, content_id: &str) -> Vec<DownloadSubscriber> {
        let mut subscribers = self.download_subscribers.write().await;
        subscribers.remove(content_id).unwrap_or_default()
    }

    /// Get subscribers count for a download (without removing)
    pub async fn get_subscriber_count(&self, content_id: &str) -> usize {
        let subscribers = self.download_subscribers.read().await;
        subscribers.get(content_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Set audio selection state for a user+episode
    pub async fn set_audio_selection(&self, key: String, state: AudioSelectionState) {
        let mut selections = self.audio_selections.write().await;
        selections.insert(key, state);
        // Limit cache size
        if selections.len() > 50 {
            if let Some(k) = selections.keys().next().cloned() {
                selections.remove(&k);
            }
        }
    }

    /// Toggle an audio selection index, returns updated state
    pub async fn toggle_audio_selection(&self, key: &str, idx: usize) -> Option<AudioSelectionState> {
        let mut selections = self.audio_selections.write().await;
        let state = selections.get_mut(key)?;
        if let Some(pos) = state.selected_indices.iter().position(|&i| i == idx) {
            // Don't allow deselecting the last one
            if state.selected_indices.len() > 1 {
                state.selected_indices.remove(pos);
            }
        } else if idx < state.versions.len() {
            state.selected_indices.push(idx);
        }
        Some(state.clone())
    }

    /// Get audio selection state
    pub async fn get_audio_selection(&self, key: &str) -> Option<AudioSelectionState> {
        let selections = self.audio_selections.read().await;
        selections.get(key).cloned()
    }

    /// Remove audio selection state
    pub async fn remove_audio_selection(&self, key: &str) {
        let mut selections = self.audio_selections.write().await;
        selections.remove(key);
    }
}

const GITHUB_REPO: &str = "AnCry1596/Crunchyroll-Downloader-Bot";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Check GitHub releases for a newer version. Returns (latest_tag, release_url) if update available.
async fn check_for_update() -> Option<(String, String)> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );
    let client = wreq::Client::builder()
        .build()
        .ok()?;
    let resp = client
        .get(&url)
        .header("User-Agent", format!("crunchyroll-downloader-bot/{}", CURRENT_VERSION))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    let html_url = json.get("html_url")?.as_str().unwrap_or("");

    // Strip leading 'v' for comparison
    let latest = tag.trim_start_matches('v');
    let current = CURRENT_VERSION.trim_start_matches('v');

    if latest != current {
        Some((tag.to_string(), html_url.to_string()))
    } else {
        None
    }
}

/// Detect the max file size allowed for direct Telegram upload based on the api_url:
/// - None (official API): 50 MB
/// - api_url behind Cloudflare (cf-ray or server: cloudflare headers): 100 MB
/// - api_url on own server: 2048 MB
async fn detect_telegram_upload_limit(api_url: Option<&str>) -> u64 {
    const MB: u64 = 1024 * 1024;
    let Some(url) = api_url else {
        tracing::info!("No custom Telegram API URL — upload limit: 50 MB");
        return 50 * MB;
    };

    // Probe the server for Cloudflare headers
    match wreq::Client::builder().build() {
        Ok(client) => {
            match client.head(url).send().await {
                Ok(resp) => {
                    let headers = resp.headers();
                    let is_cloudflare = headers.contains_key("cf-ray")
                        || headers.get("server").and_then(|v| v.to_str().ok())
                            .map(|s| s.to_lowercase().contains("cloudflare"))
                            .unwrap_or(false);
                    if is_cloudflare {
                        tracing::info!("Telegram API URL is behind Cloudflare — upload limit: 100 MB");
                        100 * MB
                    } else {
                        tracing::info!("Telegram API URL is own server — upload limit: 2048 MB");
                        2048 * MB
                    }
                }
                Err(e) => {
                    tracing::warn!("Could not probe Telegram API URL for Cloudflare detection: {} — defaulting to 2048 MB", e);
                    2048 * MB
                }
            }
        }
        Err(_) => 2048 * MB,
    }
}

/// Run the Telegram bot
pub async fn run_bot(config: Config) -> Result<()> {
    tracing::info!("Starting Telegram bot...");

    // Cleanup temp directories on startup (except tools)
    cleanup_temp_directories(&config).await;

    // Initialize tool manager and check/download tools
    let tools_dir = config.download.temp_dir.join("tools");
    let tool_manager = ToolManager::new(tools_dir);

    tracing::info!("Checking external tools...");
    if let Err(e) = tool_manager.ensure_tools().await {
        tracing::warn!("Failed to ensure tools: {}. Some features may not work.", e);
    }

    let bot = if let Some(ref api_url) = config.telegram.api_url {
        let url = Url::parse(api_url).map_err(|e| {
            crate::error::Error::Config(format!("Invalid telegram.api_url '{}': {}", api_url, e))
        })?;
        tracing::info!("Using custom Telegram API URL: {}", api_url);
        Bot::new(&config.telegram.bot_token).set_api_url(url)
    } else {
        tracing::info!("Using default Telegram API");
        Bot::new(&config.telegram.bot_token)
    };

    // Initialize Crunchyroll client with proxy support
    let cr_client = CrunchyrollClient::new(&config.crunchyroll, &config.proxy)?;

    // Initialize proxy and detect geo location
    tracing::info!("Detecting geo location for proxy configuration...");
    if let Err(e) = cr_client.init_proxy().await {
        tracing::warn!("Failed to initialize proxy: {}. Using direct connection.", e);
    }

    tracing::info!("Logging in to Crunchyroll...");
    cr_client.login().await?;
    tracing::info!("Crunchyroll login successful");

    // Initialize Widevine CDM
    let cdm = match WidevineCdm::new(&config.widevine) {
        Ok(cdm) => {
            tracing::info!("Widevine CDM initialized");
            Some(cdm)
        }
        Err(e) => {
            tracing::warn!("Failed to initialize Widevine CDM: {}. DRM content will not be available.", e);
            None
        }
    };

    // Initialize MongoDB if configured (before download manager so it can use it for key caching)
    let database = if let Some(ref uri) = config.database.connection_string {
        match Database::connect(uri, &config.database.db_name).await {
            Ok(db) => {
                tracing::info!("Connected to MongoDB");
                Some(db)
            }
            Err(e) => {
                tracing::warn!("Failed to connect to MongoDB: {}. Caching disabled.", e);
                None
            }
        }
    } else {
        tracing::info!("MongoDB not configured. File caching disabled.");
        None
    };

    // Cleanup active downloads and notify users on startup
    if let Some(ref db) = database {
        let startup_strings = Strings::get(&Lang::from_str(&config.telegram.language));
        cleanup_active_downloads_and_notify(&bot, db, startup_strings).await;
    }

    // Initialize download manager
    let http = wreq::Client::builder()
        .cookie_store(true)
        .redirect(wreq::redirect::Policy::limited(10))
        .emulation(wreq_util::Emulation::Chrome143)
        .build()
        .map_err(|e| crate::error::Error::Network(e.to_string()))?;

    let download_manager = DownloadManager::new(http, cdm, config.download.clone(), database.clone());

    // Detect Telegram upload size limit based on api_url / Cloudflare presence
    let telegram_max_upload_bytes = detect_telegram_upload_limit(config.telegram.api_url.as_deref()).await;

    // Fetch bot display name from Telegram
    let bot_name = match bot.get_me().await {
        Ok(me) => me.full_name(),
        Err(_) => "Bot".to_string(),
    };
    tracing::info!("Bot name: {}", bot_name);

    // Create shared state
    let state = Arc::new(BotState::new(
        cr_client,
        download_manager,
        tool_manager,
        config,
        database,
        telegram_max_upload_bytes,
        bot_name,
    ));

    // Spawn background update checker:
    // - Checks GitHub once per day
    // - If outdated, notifies owners every 2 minutes until updated
    {
        let bot_clone = bot.clone();
        let owner_ids: Vec<i64> = state.config.telegram.owner_users.iter().map(|o| o.id).collect();
        let owner_lang = state.config.telegram.language.clone();
        tokio::spawn(async move {
            const CHECK_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(86400); // 1 day
            const SPAM_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(120);   // 2 min

            loop {
                tracing::info!("Checking for updates (current: v{})...", CURRENT_VERSION);
                match check_for_update().await {
                    Some((tag, url)) => {
                        tracing::warn!("New version available: {} — {}", tag, url);
                        let strings = crate::i18n::Strings::get(&crate::i18n::Lang::from_str(&owner_lang));
                        let msg = strings.update_available
                            .replace("{cur}", CURRENT_VERSION)
                            .replace("{tag}", &tag)
                            .replace("{url}", &url);
                        // Spam every 2 minutes until next daily check
                        let spam_count = CHECK_INTERVAL.as_secs() / SPAM_INTERVAL.as_secs();
                        for _ in 0..spam_count {
                            for &owner_id in &owner_ids {
                                let _ = bot_clone
                                    .send_message(ChatId(owner_id), &msg)
                                    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                    .await;
                            }
                            tokio::time::sleep(SPAM_INTERVAL).await;
                        }
                    }
                    None => {
                        tracing::info!("Bot is up to date (v{})", CURRENT_VERSION);
                        tokio::time::sleep(CHECK_INTERVAL).await;
                    }
                }
            }
        });
    }

    // Set bot commands menu
    tracing::info!("Setting bot commands menu...");
    if let Err(e) = bot.set_my_commands(Command::bot_commands()).await {
        tracing::warn!("Failed to set bot commands menu: {}", e);
    } else {
        tracing::info!("Bot commands menu set successfully");
    }

    // Build handler
    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(handle_command),
        )
        .branch(
            Update::filter_message()
                .filter(|msg: Message| msg.text().is_some())
                .endpoint(handle_text_message),
        )
        .branch(Update::filter_callback_query().endpoint(handle_callback));

    // Start dispatcher
    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

/// Handle bot commands
async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    state: Arc<BotState>,
) -> ResponseResult<()> {
    let msg_id = msg.id;
    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);

    let chat_id_raw = msg.chat.id.0;

    let s = state.strings;

    // Admin commands - check permission separately
    match &cmd {
        Command::AddAdmin | Command::RemoveAdmin => {
            if !state.is_owner(user_id) {
                bot.send_message(msg.chat.id, s.owner_only)
                    .reply_parameters(ReplyParameters::new(msg_id))
                    .await?;
                return Ok(());
            }
        }
        Command::Authorize | Command::Deauthorize => {
            if !state.is_owner_or_admin(user_id).await {
                bot.send_message(msg.chat.id, s.owner_or_admin_only)
                    .reply_parameters(ReplyParameters::new(msg_id))
                    .await?;
                return Ok(());
            }
        }
        _ => {
            // Check general access permission
            if !state.is_allowed(user_id, chat_id_raw).await {
                let not_authorized_msg = build_not_authorized_message(&state.config.telegram.owner_users, s);
                bot.send_message(msg.chat.id, not_authorized_msg)
                    .reply_parameters(ReplyParameters::new(msg_id))
                    .await?;
                return Ok(());
            }
        }
    }

    match cmd {
        Command::Start => {
            let welcome_msg = build_welcome_message(&state.config.telegram.owner_users, &state.bot_name, s);
            bot.send_message(msg.chat.id, welcome_msg)
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }

        Command::Help => {
            let help_msg = build_help_message(&state.config.telegram.owner_users, &state.bot_name, s);
            bot.send_message(msg.chat.id, help_msg)
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }

        Command::Search => {
            let text = msg.text().unwrap_or_default();
            let search_term = text.strip_prefix("/search").unwrap_or("").trim();

            if search_term.is_empty() {
                bot.send_message(msg.chat.id, s.search_empty)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
            } else {
                handle_search(&bot, msg.chat.id, msg_id, search_term, user_id, &state).await?;
            }
        }

        Command::Get => {
            let text = msg.text().unwrap_or_default();
            let input = text.strip_prefix("/get").unwrap_or("").trim();

            if input.is_empty() {
                bot.send_message(msg.chat.id, s.get_empty)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
            } else {
                handle_get(&bot, msg.chat.id, msg_id, input, user_id, &state).await?;
            }
        }

        Command::Cancel => {
            state.download_manager.cancel();
            bot.send_message(msg.chat.id, s.cancelled_ok)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }

        Command::Status => {
            let cr_status = if state.cr_client.is_authenticated().await {
                s.status_connected
            } else {
                s.status_disconnected
            };

            let owner_info = if state.config.telegram.owner_users.is_empty() {
                String::new()
            } else {
                format!(
                    "\n👑 Owner: {}",
                    state.config.telegram.format_owners()
                )
            };

            let status = format!(
                "{}{}",
                s.status_message.replace("{name}", &state.bot_name).replace("{cr}", cr_status),
                owner_info
            );

            bot.send_message(msg.chat.id, status)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }

        Command::Donate => {
            let donate_msg = build_donate_message(&state.bot_name, s);
            bot.send_message(msg.chat.id, donate_msg)
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }

        Command::Stats => {
            if state.database.is_none() {
                bot.send_message(msg.chat.id, s.stats_no_db)
                    .reply_parameters(ReplyParameters::new(msg_id))
                    .await?;
                return Ok(());
            }
            if let Some(ref db) = state.database {
                match db.get_stats().await {
                    Ok(stats) => {
                        let owner_count = state.config.telegram.owner_users.len();
                        let total_authorized = owner_count as u64 + stats.admin_count + stats.authorized_chat_count;

                        // Build active downloads section with detailed progress
                        let active_downloads_section = if stats.active_downloads_list.is_empty() {
                            format!("<b>{}</b> 0\n", s.stats_downloading)
                        } else {
                            let mut section = format!("<b>{}</b> {}\n", s.stats_downloading, stats.active_downloads_count);
                            for dl in &stats.active_downloads_list {
                                let title = if let Some(ref series) = dl.series_title {
                                    format!("{} - {}", series, dl.title)
                                } else {
                                    dl.title.clone()
                                };
                                // Escape HTML special chars
                                let title = title.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");

                                // Build progress bar
                                let progress_bar = {
                                    let filled = (dl.progress as usize * 10) / 100;
                                    let empty = 10 - filled;
                                    format!("[{}{}] {}%", "█".repeat(filled), "░".repeat(empty), dl.progress)
                                };

                                // Build size info
                                let size_info = if let Some(est_size) = dl.estimated_size {
                                    format!(
                                        "📥 {:.2} MB / {:.2} MB",
                                        dl.downloaded_bytes as f64 / 1_048_576.0,
                                        est_size as f64 / 1_048_576.0
                                    )
                                } else if dl.downloaded_bytes > 0 {
                                    format!("📥 {:.2} MB", dl.downloaded_bytes as f64 / 1_048_576.0)
                                } else {
                                    String::new()
                                };

                                // Build speed info
                                let speed_info = if let Some(speed) = dl.speed {
                                    if speed >= 1_048_576 {
                                        format!("⚡ {:.2} MB/s", speed as f64 / 1_048_576.0)
                                    } else {
                                        format!("⚡ {:.2} KB/s", speed as f64 / 1024.0)
                                    }
                                } else {
                                    String::new()
                                };

                                section.push_str(&format!("\n📺 <b>{}</b>\n", title));
                                section.push_str(&format!("   {} {}\n", dl.phase, progress_bar));
                                if !size_info.is_empty() || !speed_info.is_empty() {
                                    let details: Vec<&str> = [size_info.as_str(), speed_info.as_str()]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .collect();
                                    section.push_str(&format!("   {}\n", details.join(" | ")));
                                }
                            }
                            section
                        };

                        let stats_msg = format!(
                            "<b>{}</b>\n\n\
                            {}\n\
                            <b>{}</b>\n\
                            • {}: {}\n\
                            • {}: {}\n\
                            • {}: {}\n\
                            • {}: {}\n\n\
                            <b>{}</b> {}\n\n\
                            <b>{}</b>\n\
                            • {}: {} ({:.2} GB)\n\
                            • {}: {} {}\n\n\
                            <b>{}</b>\n\
                            • {}: {} ({:.2} GB)\n\
                            • {}: {} {}\n\n\
                            <b>{}</b>\n\
                            • {}: {} ({:.2} GB)\n\
                            • {}: {} {}\n\n\
                            <b>{}</b>\n\
                            • {}: {} ({:.2} GB)\n\
                            • {}: {} {}\n\n\
                            <b>{}</b>\n\
                            • {}: {}\n\
                            • {}: {:.2} GB\n\
                            • {}: {}",
                            s.stats_header.replace("{name}", &state.bot_name),
                            active_downloads_section,
                            s.stats_allowed_users,
                            s.stats_owner, owner_count,
                            s.stats_admin, stats.admin_count,
                            s.stats_authorized_chats, stats.authorized_chat_count,
                            s.stats_total, total_authorized,
                            s.stats_episodes_decrypted, stats.episodes_decrypted,
                            s.stats_cache_telegram,
                            s.stats_files, stats.telegram_cache.file_count, stats.telegram_cache.total_size as f64 / 1_073_741_824.0,
                            s.stats_served, stats.telegram_cache.serve_count, s.stats_times,
                            s.stats_cache_buzzheavier,
                            s.stats_files, stats.buzzheavier_cache.file_count, stats.buzzheavier_cache.total_size as f64 / 1_073_741_824.0,
                            s.stats_served, stats.buzzheavier_cache.serve_count, s.stats_times,
                            s.stats_cache_pixeldrain,
                            s.stats_files, stats.pixeldrain_cache.file_count, stats.pixeldrain_cache.total_size as f64 / 1_073_741_824.0,
                            s.stats_served, stats.pixeldrain_cache.serve_count, s.stats_times,
                            s.stats_cache_gofile,
                            s.stats_files, stats.gofile_cache.file_count, stats.gofile_cache.total_size as f64 / 1_073_741_824.0,
                            s.stats_served, stats.gofile_cache.serve_count, s.stats_times,
                            s.stats_summary,
                            s.stats_total_files, stats.total_cached_files,
                            s.stats_total_size, stats.total_cached_size as f64 / 1_073_741_824.0,
                            s.stats_total_served, stats.total_serve_count
                        );

                        bot.send_message(msg.chat.id, stats_msg)
                            .parse_mode(teloxide::types::ParseMode::Html)
                            .reply_parameters(ReplyParameters::new(msg_id))
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(msg.chat.id, format!("{}: {}", s.stats_error, e))
                            .reply_parameters(ReplyParameters::new(msg_id))
                            .await?;
                    }
                }
            }
        }

        Command::Tools => {
            let statuses = state.tool_manager.check_tools().await;

            let mut text = s.tools_header.to_string();
            for status in statuses {
                let icon = if status.available { "✅" } else { "❌" };
                let version = status.version.unwrap_or_else(|| "N/A".to_string());
                let location = status
                    .path
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| s.system_path.to_string());

                text.push_str(&format!(
                    "{} *{}*\n  {}: {}\n  {}: {}\n\n",
                    icon, status.name, s.tools_version, version, s.tools_location, location
                ));
            }

            text.push_str(s.tools_install_hint);
            bot.send_message(msg.chat.id, text)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }

        Command::InstallTools => {
            let msg_sent = bot
                .send_message(msg.chat.id, s.tools_installing)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;

            match state.tool_manager.ensure_tools().await {
                Ok(()) => {
                    bot.edit_message_text(
                        msg.chat.id,
                        msg_sent.id,
                        s.tools_installed_ok,
                    )
                    .await?;
                }
                Err(e) => {
                    bot.edit_message_text(
                        msg.chat.id,
                        msg_sent.id,
                        format!("{}: {}", s.tools_install_failed, e),
                    )
                    .await?;
                }
            }
        }

        Command::AddAdmin => {
            handle_add_admin(&bot, &msg, user_id, &state).await?;
        }

        Command::RemoveAdmin => {
            handle_remove_admin(&bot, &msg, user_id, &state).await?;
        }

        Command::Authorize => {
            handle_authorize(&bot, &msg, user_id, &state).await?;
        }

        Command::Deauthorize => {
            handle_deauthorize(&bot, &msg, user_id, &state).await?;
        }
    }

    Ok(())
}

/// Handle text messages (for search without command prefix)
async fn handle_text_message(
    _bot: Bot,
    msg: Message,
    state: Arc<BotState>,
) -> ResponseResult<()> {
    // Check if user is allowed
    if let Some(ref user) = msg.from {
        let chat_id_raw = msg.chat.id.0;
        if !state.is_allowed(user.id.0 as i64, chat_id_raw).await {
            return Ok(());
        }
    }

    // Could implement conversational search or unsubscribe handling here
    Ok(())
}

/// Handle search
async fn handle_search(
    bot: &Bot,
    chat_id: ChatId,
    reply_to: MessageId,
    query: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    let searching_msg = bot
        .send_message(chat_id, format!("{} '{}'...", s.searching, query))
        .reply_parameters(ReplyParameters::new(reply_to))
        .await?;

    match state.cr_client.search(query, 10).await {
        Ok(results) => {
            if results.is_empty() {
                bot.edit_message_text(
                    chat_id,
                    searching_msg.id,
                    format!("{} '{}'", s.search_not_found, query),
                )
                .await?;
                return Ok(());
            }

            let keyboard = search_results_keyboard(&results, user_id);
            bot.edit_message_text(
                chat_id,
                searching_msg.id,
                format!("{} {} results for '{}':", s.search_found, results.len(), query),
            )
            .reply_markup(keyboard)
            .await?;
        }
        Err(e) => {
            bot.edit_message_text(
                chat_id,
                searching_msg.id,
                format!("{}: {}", s.search_error, e),
            )
            .await?;
        }
    }

    Ok(())
}

// /// Handle /get command for direct ID or URL input
async fn handle_get(
    bot: &Bot,
    chat_id: ChatId,
    reply_to: MessageId,
    input: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    let loading_msg = bot
        .send_message(chat_id, format!("{} '{}'...", s.get_loading, input))
        .reply_parameters(ReplyParameters::new(reply_to))
        .await?;

    let content_type = match parse_crunchyroll_input(input) {
        Some(ct) => ct,
        None => {
            bot.edit_message_text(chat_id, loading_msg.id, s.get_invalid_input)
            .await?;
            return Ok(());
        }
    };

    match content_type {
        ContentType::Series(id) => {
            handle_get_series(bot, chat_id, loading_msg.id, &id, user_id, state).await?;
        }
        ContentType::Episode(id) => {
            handle_get_content(bot, chat_id, loading_msg.id, &id, user_id, state).await?;
        }
        ContentType::Season(id) => {
            handle_get_season(bot, chat_id, loading_msg.id, &id, user_id, state).await?;
        }
        ContentType::MovieListing(id) => {
            handle_get_movie_listing(bot, chat_id, loading_msg.id, &id, user_id, state).await?;
        }
        ContentType::Movie(id) => {
            handle_get_movie(bot, chat_id, loading_msg.id, &id, user_id, state).await?;
        }
    }

    Ok(())
}

async fn handle_get_series(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: teloxide::types::MessageId,
    series_id: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    match state.cr_client.get_series(series_id).await {
        Ok(series) => {
            match state.cr_client.get_seasons(series_id).await {
                Ok(seasons) => {
                    if seasons.is_empty() {
                        bot.edit_message_text(chat_id, msg_id, s.no_seasons).await?;
                        return Ok(());
                    }

                    let keyboard = seasons_keyboard(&seasons, series_id, user_id, s);
                    bot.edit_message_text(
                        chat_id,
                        msg_id,
                        format!("📺 *{}*\n\n{}", series.title, s.seasons_select),
                    )
                    .reply_markup(keyboard)
                    .await?;
                }
                Err(e) => {
                    bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.seasons_error, e)).await?;
                }
            }
        }
        Err(_) => {
            bot.edit_message_text(chat_id, msg_id, format!("{} '{}'", s.get_not_found, series_id)).await?;
        }
    }
    Ok(())
}

async fn handle_get_content(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: teloxide::types::MessageId,
    id: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    if let Ok(episode) = state.cr_client.get_episode(id).await {
        let series_title = episode.series_title.as_deref().unwrap_or(s.unknown_field);
        let season_title = episode.season_title.as_deref().unwrap_or(s.unknown_field);
        let episode_title = &episode.title;

        let info = format!(
            "📺 {} | 📁 {}\n\n\
            🎬 {}\n\n\
            {}: {}\n\
            {}: {}\n\
            {}: {}\n\n\
            📝 {}",
            series_title,
            season_title,
            episode_title,
            s.field_episode, episode.display_number(),
            s.field_duration, episode.duration_formatted(),
            s.field_audio, episode.audio_locale.as_deref().unwrap_or(s.unknown_field),
            episode.description.as_deref().unwrap_or(s.no_description),
        );

        let pixeldrain_enabled = state.config.download.pixeldrain_api_key.is_some();
        let keyboard = episode_actions_keyboard_with_pixeldrain(id, episode.season_id.as_deref().unwrap_or(""), pixeldrain_enabled, user_id, s);
        bot.edit_message_text(chat_id, msg_id, info)
            .reply_markup(keyboard)
            .await?;
        return Ok(());
    }

    if let Ok(series) = state.cr_client.get_series(id).await {
        match state.cr_client.get_seasons(id).await {
            Ok(seasons) => {
                if seasons.is_empty() {
                    bot.edit_message_text(chat_id, msg_id, s.no_seasons).await?;
                    return Ok(());
                }

                let keyboard = seasons_keyboard(&seasons, id, user_id, s);
                bot.edit_message_text(
                    chat_id,
                    msg_id,
                    format!("📺 *{}*\n\n{}", series.title, s.seasons_select),
                )
                .reply_markup(keyboard)
                .await?;
                return Ok(());
            }
            Err(e) => {
                bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.seasons_error, e)).await?;
                return Ok(());
            }
        }
    }

    if let Ok(_movie_listing) = state.cr_client.get_movie_listing(id).await {
        match state.cr_client.get_movies(id).await {
            Ok(movies) => {
                if movies.is_empty() {
                    bot.edit_message_text(chat_id, msg_id, s.no_movies).await?;
                    return Ok(());
                }

                let movie = &movies[0];
                let info = format!(
                    "🎬 *{}*\n\n\
                    {}: {}\n\
                    {}: {}\n\n\
                    📝 {}",
                    movie.title,
                    s.field_duration, movie.duration_formatted(),
                    s.field_audio, movie.audio_locale.as_deref().unwrap_or(s.unknown_field),
                    movie.description.as_deref().unwrap_or(s.no_description),
                );

                let keyboard = episode_actions_keyboard(&movie.id, "", user_id, s);
                bot.edit_message_text(chat_id, msg_id, info)
                    .reply_markup(keyboard)
                    .await?;
                return Ok(());
            }
            Err(e) => {
                bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.movie_error, e)).await?;
                return Ok(());
            }
        }
    }

    if let Ok(movie) = state.cr_client.get_movie(id).await {
        let info = format!(
            "🎬 *{}*\n\n\
            {}: {}\n\
            {}: {}\n\n\
            📝 {}",
            movie.title,
            s.field_duration, movie.duration_formatted(),
            s.field_audio, movie.audio_locale.as_deref().unwrap_or(s.unknown_field),
            movie.description.as_deref().unwrap_or(s.no_description),
        );

        let keyboard = episode_actions_keyboard(id, "", user_id, s);
        bot.edit_message_text(chat_id, msg_id, info)
            .reply_markup(keyboard)
            .await?;
        return Ok(());
    }

    bot.edit_message_text(chat_id, msg_id, format!("{} '{}'.", s.get_not_found, id)).await?;

    Ok(())
}

async fn handle_get_season(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: teloxide::types::MessageId,
    season_id: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
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
            let series_id = episodes.first()
                .and_then(|ep| ep.series_id.clone())
                .unwrap_or_default();

            let keyboard = episodes_keyboard(&episodes, season_id, &series_id, 0, 8, user_id, s);
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

async fn handle_get_movie_listing(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: teloxide::types::MessageId,
    movie_listing_id: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    match state.cr_client.get_movies(movie_listing_id).await {
        Ok(movies) => {
            if movies.is_empty() {
                bot.edit_message_text(chat_id, msg_id, s.no_movies).await?;
                return Ok(());
            }

            let movie = &movies[0];
            let info = format!(
                "🎬 *{}*\n\n\
                {}: {}\n\
                {}: {}\n\n\
                📝 {}",
                movie.title,
                s.field_duration, movie.duration_formatted(),
                s.field_audio, movie.audio_locale.as_deref().unwrap_or(s.unknown_field),
                movie.description.as_deref().unwrap_or(s.no_description),
            );

            let keyboard = episode_actions_keyboard(&movie.id, "", user_id, s);
            bot.edit_message_text(chat_id, msg_id, info)
                .reply_markup(keyboard)
                .await?;
        }
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.movie_error, e)).await?;
        }
    }
    Ok(())
}

async fn handle_get_movie(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: teloxide::types::MessageId,
    movie_id: &str,
    user_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    match state.cr_client.get_movie(movie_id).await {
        Ok(movie) => {
            let info = format!(
                "🎬 *{}*\n\n\
                {}: {}\n\
                {}: {}\n\n\
                📝 {}",
                movie.title,
                s.field_duration, movie.duration_formatted(),
                s.field_audio, movie.audio_locale.as_deref().unwrap_or(s.unknown_field),
                movie.description.as_deref().unwrap_or(s.no_description),
            );

            let keyboard = episode_actions_keyboard(movie_id, "", user_id, s);
            bot.edit_message_text(chat_id, msg_id, info)
                .reply_markup(keyboard)
                .await?;
        }
        Err(e) => {
            bot.edit_message_text(chat_id, msg_id, format!("{}: {}", s.movie_not_found, e)).await?;
        }
    }
    Ok(())
}

/// Cleanup temp directories on startup (except tools folder)
async fn cleanup_temp_directories(config: &Config) {
    tracing::info!("🧹 Cleaning up temp directories on startup...");

    // Cleanup temp dir (except tools folder)
    let temp_dir = &config.download.temp_dir;
    if temp_dir.exists() {
        match tokio::fs::read_dir(temp_dir).await {
            Ok(mut entries) => {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    // Skip the tools folder
                    if path.file_name().map(|n| n == "tools").unwrap_or(false) {
                        tracing::info!("  ⏭️ Skipping tools folder");
                        continue;
                    }

                    if path.is_dir() {
                        if let Err(e) = tokio::fs::remove_dir_all(&path).await {
                            tracing::warn!("  ⚠️ Failed to remove temp folder {:?}: {}", path, e);
                        } else {
                            tracing::info!("  🗑️ Removed temp folder: {:?}", path);
                        }
                    } else if let Err(e) = tokio::fs::remove_file(&path).await {
                        tracing::warn!("  ⚠️ Failed to remove temp file {:?}: {}", path, e);
                    } else {
                        tracing::info!("  🗑️ Removed temp file: {:?}", path);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read temp directory: {}", e);
            }
        }
    }

    // Cleanup downloads folder
    let downloads_dir = &config.download.output_dir;
    if downloads_dir.exists() {
        match tokio::fs::read_dir(downloads_dir).await {
            Ok(mut entries) => {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_dir() {
                        if let Err(e) = tokio::fs::remove_dir_all(&path).await {
                            tracing::warn!("  ⚠️ Failed to remove download folder {:?}: {}", path, e);
                        } else {
                            tracing::info!("  🗑️ Removed download folder: {:?}", path);
                        }
                    } else if let Err(e) = tokio::fs::remove_file(&path).await {
                        tracing::warn!("  ⚠️ Failed to remove download file {:?}: {}", path, e);
                    } else {
                        tracing::info!("  🗑️ Removed download file: {:?}", path);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read downloads directory: {}", e);
            }
        }
    }

    tracing::info!("✅ Temp directories cleanup completed");
}

/// Cleanup active downloads and notify users that bot has restarted
async fn cleanup_active_downloads_and_notify(bot: &Bot, db: &Database, strings: &'static crate::i18n::Strings) {
    tracing::info!("🧹 Cleaning up active downloads from previous session...");

    // Get all active downloads before clearing
    match db.get_all_active_downloads().await {
        Ok(active_downloads) => {
            if active_downloads.is_empty() {
                tracing::info!("  ✅ No active downloads to cleanup");
                return;
            }

            tracing::info!("  📋 Found {} active downloads to cleanup", active_downloads.len());

            // Notify each user that their download was interrupted
            for download in &active_downloads {
                let message = strings.bot_restarted
                    .replace("{title}", &download.title)
                    .replace("{phase}", &download.phase)
                    .replace("{pct}", &download.progress.to_string());

                if let Err(e) = bot.send_message(ChatId(download.initiated_by), message).await {
                    tracing::warn!(
                        "  ⚠️ Failed to notify user {} about interrupted download: {}",
                        download.initiated_by, e
                    );
                } else {
                    tracing::info!(
                        "  📤 Notified user {} about interrupted download: {}",
                        download.initiated_by, download.title
                    );
                }
            }

            // Clear all active downloads
            match db.clear_all_active_downloads().await {
                Ok(count) => {
                    tracing::info!("  🗑️ Cleared {} active downloads from database", count);
                }
                Err(e) => {
                    tracing::warn!("  ⚠️ Failed to clear active downloads: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::warn!("  ⚠️ Failed to get active downloads: {}", e);
        }
    }

    tracing::info!("✅ Active downloads cleanup completed");
}

/// Extract target user_id from command - either from reply or from argument text
fn extract_target_user(msg: &Message) -> Option<(i64, Option<String>)> {
    // First check if replying to a message
    if let Some(reply) = msg.reply_to_message() {
        if let Some(user) = &reply.from {
            let username = user.username.clone();
            return Some((user.id.0 as i64, username));
        }
    }

    // Otherwise parse from command text
    let text = msg.text().unwrap_or_default();
    let parts: Vec<&str> = text.splitn(2, ' ').collect();
    if parts.len() > 1 {
        let arg = parts[1].trim();
        if let Ok(uid) = arg.parse::<i64>() {
            return Some((uid, None));
        }
    }

    None
}

/// Handle /addadmin command (owner only)
async fn handle_add_admin(
    bot: &Bot,
    msg: &Message,
    owner_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    let msg_id = msg.id;
    let db = match &state.database {
        Some(db) => db,
        None => {
            bot.send_message(msg.chat.id, s.db_not_configured)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
            return Ok(());
        }
    };

    let (target_id, username) = match extract_target_user(msg) {
        Some(t) => t,
        None => {
            bot.send_message(msg.chat.id, s.add_admin_usage)
            .reply_parameters(ReplyParameters::new(msg_id))
            .await?;
            return Ok(());
        }
    };

    if state.is_owner(target_id) {
        bot.send_message(msg.chat.id, format!("{} {}", s.already_owner, target_id))
            .reply_parameters(ReplyParameters::new(msg_id))
            .await?;
        return Ok(());
    }

    if db.is_admin(target_id).await.unwrap_or(false) {
        bot.send_message(msg.chat.id, format!("{} {}", s.already_admin, target_id))
            .reply_parameters(ReplyParameters::new(msg_id))
            .await?;
        return Ok(());
    }

    let admin = AdminUser::new(target_id, username.clone(), owner_id);
    match db.add_admin(&admin).await {
        Ok(_) => {
            let display = username.map(|u| format!("@{} ({})", u, target_id))
                .unwrap_or_else(|| target_id.to_string());
            bot.send_message(msg.chat.id, format!("{}: {}", s.admin_added, display))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("{}: {}", s.admin_add_error, e))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
    }

    Ok(())
}

/// Handle /removeadmin command (owner only)
async fn handle_remove_admin(
    bot: &Bot,
    msg: &Message,
    _owner_id: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    let msg_id = msg.id;
    let db = match &state.database {
        Some(db) => db,
        None => {
            bot.send_message(msg.chat.id, s.db_not_configured)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
            return Ok(());
        }
    };

    let (target_id, username) = match extract_target_user(msg) {
        Some(t) => t,
        None => {
            bot.send_message(msg.chat.id, s.remove_admin_usage)
            .reply_parameters(ReplyParameters::new(msg_id))
            .await?;
            return Ok(());
        }
    };

    match db.remove_admin(target_id).await {
        Ok(true) => {
            let display = username.map(|u| format!("@{} ({})", u, target_id))
                .unwrap_or_else(|| target_id.to_string());
            bot.send_message(msg.chat.id, format!("{}: {}", s.admin_removed, display))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
        Ok(false) => {
            bot.send_message(msg.chat.id, format!("{} {}", s.admin_not_found, target_id))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("{}: {}", s.admin_remove_error, e))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
    }

    Ok(())
}

/// Handle /authorize command (owner + admin)
async fn handle_authorize(
    bot: &Bot,
    msg: &Message,
    authorized_by: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    let msg_id = msg.id;
    let db = match &state.database {
        Some(db) => db,
        None => {
            bot.send_message(msg.chat.id, s.db_not_configured)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
            return Ok(());
        }
    };

    let text = msg.text().unwrap_or_default();
    let parts: Vec<&str> = text.splitn(2, ' ').collect();

    let (target_chat_id, title) = if parts.len() > 1 {
        let arg = parts[1].trim();
        match arg.parse::<i64>() {
            Ok(id) => (id, None),
            Err(_) => {
                bot.send_message(msg.chat.id, s.authorize_usage)
                    .reply_parameters(ReplyParameters::new(msg_id))
                    .await?;
                return Ok(());
            }
        }
    } else {
        let chat_title = if msg.chat.id.0 < 0 {
            msg.chat.title().map(|t| t.to_string())
        } else {
            msg.chat.username().map(|u| format!("@{}", u))
        };
        (msg.chat.id.0, chat_title)
    };

    if db.is_chat_authorized(target_chat_id).await.unwrap_or(false) {
        bot.send_message(msg.chat.id, format!("{} {}", s.already_authorized, target_chat_id))
            .reply_parameters(ReplyParameters::new(msg_id))
            .await?;
        return Ok(());
    }

    let chat = AuthorizedChat::new(target_chat_id, title.clone(), authorized_by);
    match db.authorize_chat(&chat).await {
        Ok(_) => {
            let display = title
                .map(|t| format!("{} ({})", t, target_chat_id))
                .unwrap_or_else(|| target_chat_id.to_string());
            bot.send_message(msg.chat.id, format!("{}: {}", s.authorized_ok, display))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("{}: {}", s.authorize_error, e))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
    }

    Ok(())
}

/// Handle /deauthorize command (owner + admin)
async fn handle_deauthorize(
    bot: &Bot,
    msg: &Message,
    _authorized_by: i64,
    state: &BotState,
) -> ResponseResult<()> {
    let s = state.strings;
    let msg_id = msg.id;
    let db = match &state.database {
        Some(db) => db,
        None => {
            bot.send_message(msg.chat.id, s.db_not_configured)
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
            return Ok(());
        }
    };

    let text = msg.text().unwrap_or_default();
    let parts: Vec<&str> = text.splitn(2, ' ').collect();

    let target_chat_id = if parts.len() > 1 {
        let arg = parts[1].trim();
        match arg.parse::<i64>() {
            Ok(id) => id,
            Err(_) => {
                bot.send_message(msg.chat.id, s.deauthorize_usage)
                    .reply_parameters(ReplyParameters::new(msg_id))
                    .await?;
                return Ok(());
            }
        }
    } else {
        msg.chat.id.0
    };

    match db.deauthorize_chat(target_chat_id).await {
        Ok(true) => {
            bot.send_message(msg.chat.id, format!("{}: {}", s.deauthorized_ok, target_chat_id))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
        Ok(false) => {
            bot.send_message(msg.chat.id, format!("{} {}", s.not_authorized_chat, target_chat_id))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("{}: {}", s.deauthorize_error, e))
                .reply_parameters(ReplyParameters::new(msg_id))
                .await?;
        }
    }

    Ok(())
}