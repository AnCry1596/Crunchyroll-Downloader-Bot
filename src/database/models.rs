use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Admin user - can authorize chats and use the bot in private chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUser {
    /// Telegram user ID
    #[serde(rename = "_id")]
    pub user_id: i64,
    /// Username (for display)
    pub username: Option<String>,
    /// Who added this admin
    pub added_by: i64,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub added_at: DateTime<Utc>,
}

impl AdminUser {
    pub fn new(user_id: i64, username: Option<String>, added_by: i64) -> Self {
        Self {
            user_id,
            username,
            added_at: Utc::now(),
            added_by,
        }
    }
}

/// Authorized chat - a chat (group or private) where the bot is allowed to operate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedChat {
    /// Telegram chat ID (negative for groups, positive for private)
    #[serde(rename = "_id")]
    pub chat_id: i64,
    /// Chat title (for groups) or username (for private)
    pub title: Option<String>,
    /// Who authorized this chat
    pub authorized_by: i64,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub authorized_at: DateTime<Utc>,
}

impl AuthorizedChat {
    pub fn new(chat_id: i64, title: Option<String>, authorized_by: i64) -> Self {
        Self {
            chat_id,
            title,
            authorized_at: Utc::now(),
            authorized_by,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFile {
    #[serde(rename = "_id")]
    pub content_id: String,
    pub file_id: String,
    pub filename: String,
    pub file_size: u64,
    pub resolution: Option<String>,
    pub bitrate: Option<u64>,
    pub series_title: Option<String>,
    #[serde(default)]
    pub season_title: Option<String>,
    pub episode_number: Option<String>,
    pub episode_title: String,
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub audio_locales: Option<Vec<String>>,
    #[serde(default)]
    pub subtitle_locales: Option<Vec<String>>,

    // Thêm #[serde(default)] để đọc được data cũ
    #[serde(default)]
    pub message_id: Option<i32>,
    #[serde(default)]
    pub storage_chat_id: Option<i64>,

    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub cached_at: DateTime<Utc>,
    #[serde(default)]
    pub forward_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedKey {
    #[serde(rename = "_id")]
    pub pssh: String,
    pub content_id: Option<String>,
    pub keys: Vec<KeyPair>,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub fetched_at: DateTime<Utc>,
    pub use_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPair {
    pub kid: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRequest {
    #[serde(rename = "_id")]
    pub request_id: String,
    pub user_id: i64,
    pub username: Option<String>,
    pub content_id: String,
    pub content_type: String,
    pub title: String,
    pub series_title: Option<String>,
    pub status: RequestStatus,
    pub from_cache: bool,
    pub error: Option<String>,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub requested_at: DateTime<Utc>,
    #[serde(default, with = "crate::database::optional_datetime")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    Pending,
    Downloading,
    Uploading,
    Completed,
    Failed,
    Cached,
}

impl CachedFile {
    pub fn new(
        content_id: String,
        file_id: String,
        filename: String,
        file_size: u64,
        episode_title: String,
        message_id: i32,
        storage_chat_id: i64,
    ) -> Self {
        Self {
            content_id,
            file_id,
            filename,
            file_size,
            resolution: None,
            bitrate: None,
            series_title: None,
            season_title: None,
            episode_number: None,
            episode_title,
            audio_locale: None,
            audio_locales: None,
            subtitle_locales: None,
            message_id: Some(message_id),
            storage_chat_id: Some(storage_chat_id),
            cached_at: Utc::now(),
            forward_count: 0,
        }
    }

    pub fn with_video_info(mut self, resolution: String, bitrate: u64) -> Self {
        self.resolution = Some(resolution);
        self.bitrate = Some(bitrate);
        self
    }

    pub fn with_series_info(mut self, series_title: String, season_title: Option<String>, episode_number: String) -> Self {
        self.series_title = Some(series_title);
        self.season_title = season_title;
        self.episode_number = Some(episode_number);
        self
    }

    pub fn with_audio_info(mut self, audio_locale: Option<String>, subtitle_locales: Vec<String>) -> Self {
        self.audio_locale = audio_locale;
        self.subtitle_locales = if subtitle_locales.is_empty() { None } else { Some(subtitle_locales) };
        self
    }

    pub fn with_audio_locales(mut self, audio_locales: Vec<String>) -> Self {
        self.audio_locales = if audio_locales.is_empty() { None } else { Some(audio_locales) };
        self
    }
}

impl CachedKey {
    pub fn new(pssh: String, keys: Vec<KeyPair>) -> Self {
        Self {
            pssh,
            content_id: None,
            keys,
            fetched_at: Utc::now(),
            use_count: 0,
        }
    }

    pub fn with_content_id(mut self, content_id: String) -> Self {
        self.content_id = Some(content_id);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPixeldrainFile {
    #[serde(rename = "_id")]
    pub content_id: String,
    pub pixeldrain_id: String,
    pub download_url: String,
    pub filename: String,
    pub file_size: u64,
    pub series_title: Option<String>,
    pub episode_number: Option<String>,
    pub episode_title: String,
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub audio_locales: Option<Vec<String>>,
    #[serde(default)]
    pub subtitle_locales: Option<Vec<String>>,
    pub decryption_keys: Vec<KeyPair>,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub cached_at: DateTime<Utc>,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub expires_at: DateTime<Utc>,
    pub serve_count: u32,
}

impl CachedPixeldrainFile {
    pub fn new(
        content_id: String,
        pixeldrain_id: String,
        download_url: String,
        filename: String,
        file_size: u64,
        episode_title: String,
        decryption_keys: Vec<KeyPair>,
    ) -> Self {
        let now = Utc::now();
        Self {
            content_id,
            pixeldrain_id,
            download_url,
            filename,
            file_size,
            series_title: None,
            episode_number: None,
            episode_title,
            audio_locale: None,
            audio_locales: None,
            subtitle_locales: None,
            decryption_keys,
            cached_at: now,
            expires_at: now + chrono::Duration::days(60),
            serve_count: 1,
        }
    }

    pub fn with_series_info(mut self, series_title: String, episode_number: String) -> Self {
        self.series_title = Some(series_title);
        self.episode_number = Some(episode_number);
        self
    }

    pub fn with_audio_info(mut self, audio_locale: Option<String>, subtitle_locales: Vec<String>) -> Self {
        self.audio_locale = audio_locale;
        self.subtitle_locales = if subtitle_locales.is_empty() { None } else { Some(subtitle_locales) };
        self
    }

    pub fn with_audio_locales(mut self, audio_locales: Vec<String>) -> Self {
        self.audio_locales = if audio_locales.is_empty() { None } else { Some(audio_locales) };
        self
    }

    pub fn is_valid(&self) -> bool {
        Utc::now() < self.expires_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedBuzzheavierFile {
    #[serde(rename = "_id")]
    pub content_id: String,
    pub buzzheavier_id: String,
    pub download_url: String,
    pub filename: String,
    pub file_size: u64,
    pub series_title: Option<String>,
    pub episode_number: Option<String>,
    pub episode_title: String,
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub audio_locales: Option<Vec<String>>,
    #[serde(default)]
    pub subtitle_locales: Option<Vec<String>>,
    pub decryption_keys: Vec<KeyPair>,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub cached_at: DateTime<Utc>,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub expires_at: DateTime<Utc>,
    pub serve_count: u32,
}

impl CachedBuzzheavierFile {
    pub fn new(
        content_id: String,
        buzzheavier_id: String,
        download_url: String,
        filename: String,
        file_size: u64,
        episode_title: String,
        decryption_keys: Vec<KeyPair>,
    ) -> Self {
        let now = Utc::now();
        Self {
            content_id,
            buzzheavier_id,
            download_url,
            filename,
            file_size,
            series_title: None,
            episode_number: None,
            episode_title,
            audio_locale: None,
            audio_locales: None,
            subtitle_locales: None,
            decryption_keys,
            cached_at: now,
            expires_at: now + chrono::Duration::days(8),
            serve_count: 1,
        }
    }

    pub fn with_series_info(mut self, series_title: String, episode_number: String) -> Self {
        self.series_title = Some(series_title);
        self.episode_number = Some(episode_number);
        self
    }

    pub fn with_audio_info(mut self, audio_locale: Option<String>, subtitle_locales: Vec<String>) -> Self {
        self.audio_locale = audio_locale;
        self.subtitle_locales = if subtitle_locales.is_empty() { None } else { Some(subtitle_locales) };
        self
    }

    pub fn with_audio_locales(mut self, audio_locales: Vec<String>) -> Self {
        self.audio_locales = if audio_locales.is_empty() { None } else { Some(audio_locales) };
        self
    }

    pub fn is_valid(&self) -> bool {
        Utc::now() < self.expires_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedGofileFile {
    #[serde(rename = "_id")]
    pub content_id: String,
    pub gofile_file_code: String,
    pub download_url: String,
    pub filename: String,
    pub file_size: u64,
    pub series_title: Option<String>,
    pub episode_number: Option<String>,
    pub episode_title: String,
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub audio_locales: Option<Vec<String>>,
    #[serde(default)]
    pub subtitle_locales: Option<Vec<String>>,
    pub decryption_keys: Vec<KeyPair>,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub cached_at: DateTime<Utc>,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub expires_at: DateTime<Utc>,
    pub serve_count: u32,
}

impl CachedGofileFile {
    pub fn new(
        content_id: String,
        gofile_file_code: String,
        download_url: String,
        filename: String,
        file_size: u64,
        episode_title: String,
        decryption_keys: Vec<KeyPair>,
    ) -> Self {
        let now = Utc::now();
        Self {
            content_id,
            gofile_file_code,
            download_url,
            filename,
            file_size,
            series_title: None,
            episode_number: None,
            episode_title,
            audio_locale: None,
            audio_locales: None,
            subtitle_locales: None,
            decryption_keys,
            cached_at: now,
            expires_at: now + chrono::Duration::days(10),
            serve_count: 1,
        }
    }

    pub fn with_series_info(mut self, series_title: String, episode_number: String) -> Self {
        self.series_title = Some(series_title);
        self.episode_number = Some(episode_number);
        self
    }

    pub fn with_audio_info(mut self, audio_locale: Option<String>, subtitle_locales: Vec<String>) -> Self {
        self.audio_locale = audio_locale;
        self.subtitle_locales = if subtitle_locales.is_empty() { None } else { Some(subtitle_locales) };
        self
    }

    pub fn with_audio_locales(mut self, audio_locales: Vec<String>) -> Self {
        self.audio_locales = if audio_locales.is_empty() { None } else { Some(audio_locales) };
        self
    }

    pub fn is_valid(&self) -> bool {
        Utc::now() < self.expires_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveDownload {
    #[serde(rename = "_id")]
    pub content_id: String,
    pub title: String,
    pub series_title: Option<String>,
    pub initiated_by: i64,
    pub phase: String,
    pub progress: u8,
    pub estimated_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub speed: Option<u64>,
    #[serde(with = "::bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub started_at: DateTime<Utc>,
    pub use_external_upload: bool,
}

impl ActiveDownload {
    pub fn new(
        content_id: String,
        title: String,
        initiated_by: i64,
        use_external_upload: bool,
    ) -> Self {
        Self {
            content_id,
            title,
            series_title: None,
            initiated_by,
            phase: "starting".to_string(),
            progress: 0,
            estimated_size: None,
            downloaded_bytes: 0,
            speed: None,
            started_at: Utc::now(),
            use_external_upload,
        }
    }

    pub fn with_series_title(mut self, series_title: String) -> Self {
        self.series_title = Some(series_title);
        self
    }

    pub fn format_status(&self) -> String {
        let size_info = if let Some(size) = self.estimated_size {
            format!(
                "\nDownloaded: {:.2} MB / {:.2} MB",
                self.downloaded_bytes as f64 / 1_048_576.0,
                size as f64 / 1_048_576.0
            )
        } else if self.downloaded_bytes > 0 {
            format!(
                "\nDownloaded: {:.2} MB",
                self.downloaded_bytes as f64 / 1_048_576.0
            )
        } else {
            String::new()
        };

        let speed_info = if let Some(speed) = self.speed {
            format!("\nSpeed: {:.2} MB/s", speed as f64 / 1_048_576.0)
        } else {
            String::new()
        };

        format!(
            "{}{}{}",
            self.phase,
            size_info,
            speed_info
        )
    }
}

impl DownloadRequest {
    pub fn new(
        request_id: String,
        user_id: i64,
        content_id: String,
        content_type: String,
        title: String,
    ) -> Self {
        Self {
            request_id,
            user_id,
            username: None,
            content_id,
            content_type,
            title,
            series_title: None,
            status: RequestStatus::Pending,
            from_cache: false,
            error: None,
            requested_at: Utc::now(),
            completed_at: None,
        }
    }

    pub fn with_username(mut self, username: String) -> Self {
        self.username = Some(username);
        self
    }

    pub fn with_series_title(mut self, series_title: String) -> Self {
        self.series_title = Some(series_title);
        self
    }

    pub fn mark_completed(mut self) -> Self {
        self.status = RequestStatus::Completed;
        self.completed_at = Some(Utc::now());
        self
    }

    pub fn mark_cached(mut self) -> Self {
        self.status = RequestStatus::Cached;
        self.from_cache = true;
        self.completed_at = Some(Utc::now());
        self
    }

    pub fn mark_failed(mut self, error: String) -> Self {
        self.status = RequestStatus::Failed;
        self.error = Some(error);
        self.completed_at = Some(Utc::now());
        self
    }
}