use crate::error::{Error, Result};
use serde::{Deserialize, Deserializer};
use std::path::PathBuf;

/// Owner user with ID and optional username
#[derive(Debug, Clone)]
pub struct OwnerUser {
    pub id: i64,
    pub username: Option<String>,
}

impl OwnerUser {
    /// Parse from string format "id" or "id:@username"
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        if let Some((id_str, username)) = s.split_once(':') {
            let id = id_str.trim().parse().ok()?;
            let username = username.trim().to_string();
            let username = if username.is_empty() { None } else { Some(username) };
            Some(OwnerUser { id, username })
        } else {
            let id = s.parse().ok()?;
            Some(OwnerUser { id, username: None })
        }
    }

    /// Format for display - show username if available, otherwise ID
    pub fn display(&self) -> String {
        if let Some(ref username) = self.username {
            username.clone()
        } else {
            format!("`{}`", self.id)
        }
    }
}

/// Custom deserializer for owner_users that supports both formats:
/// - Simple: [123456789, 987654321]
/// - Combined: ["123456789:@username1", "987654321:@username2"]
/// - Mixed: [123456789, "987654321:@username2"]
fn deserialize_owner_users<'de, D>(deserializer: D) -> std::result::Result<Vec<OwnerUser>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OwnerEntry {
        Id(i64),
        Combined(String),
    }

    let entries: Vec<OwnerEntry> = Vec::deserialize(deserializer)?;
    let mut owners = Vec::new();

    for entry in entries {
        match entry {
            OwnerEntry::Id(id) => {
                owners.push(OwnerUser { id, username: None });
            }
            OwnerEntry::Combined(s) => {
                if let Some(owner) = OwnerUser::parse(&s) {
                    owners.push(owner);
                }
            }
        }
    }

    Ok(owners)
}

/// Upload preference for completed downloads
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum UploadPreference {
    /// Upload to Telegram first, fallback to service if file > 1.99GB
    #[default]
    Telegram,
    /// Always upload to external service (Buzzheavier/Pixeldrain/Gofile)
    Service,
}

/// Preferred upload service when using external services
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PreferredUploadService {
    /// Use Buzzheavier first (default), fallback to others
    #[default]
    Buzzheavier,
    /// Use Pixeldrain first, fallback to others
    Pixeldrain,
    /// Use Gofile first, fallback to others
    Gofile,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub crunchyroll: CrunchyrollConfig,
    pub download: DownloadConfig,
    pub widevine: WidevineConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub proxy: ProxyConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    /// List of owner users - full control over the bot
    /// Supports formats: [123456] or ["123456:@username"]
    /// Owners can add/remove admins, authorize chats, and use the bot anywhere
    #[serde(default, deserialize_with = "deserialize_owner_users")]
    pub owner_users: Vec<OwnerUser>,
    /// Chat ID of the group/channel used to store uploaded files for caching
    #[serde(default)]
    pub storage_chat_id: Option<i64>,
    /// Custom Telegram Bot API URL (for local Bot API server)
    /// If not set, uses the default Telegram API
    #[serde(default)]
    pub api_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DatabaseConfig {
    /// MongoDB connection string
    #[serde(default, alias = "mongodb_uri")]
    pub connection_string: Option<String>,
    /// Database name
    #[serde(default = "default_db_name")]
    pub db_name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProxyConfig {
    /// Main proxy URL - if set, ALL traffic uses this instead of direct connection
    /// Use this if your server IP is blocked or you want all traffic proxied
    #[serde(default)]
    pub main_proxy: Option<String>,
    /// US proxy URL for geo-restricted content
    #[serde(default)]
    pub us_proxy: Option<String>,
    /// SEA proxy URL for default region
    #[serde(default)]
    pub sea_proxy: Option<String>,
}

fn default_db_name() -> String {
    "CRBot".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct CrunchyrollConfig {
    pub email: String,
    pub password: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default = "default_audio_preferences")]
    pub preferred_audio: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadConfig {
    #[serde(default = "default_temp_dir")]
    pub temp_dir: PathBuf,
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    #[serde(default = "default_concurrent_segments")]
    pub max_concurrent_segments: usize,
    /// Upload preference: "telegram" (default) or "service"
    /// - telegram: Upload to Telegram, fallback to service if file > 1.99GB
    /// - service: Always upload to external service (Buzzheavier/Pixeldrain)
    #[serde(default)]
    pub upload_preference: UploadPreference,
    /// Preferred upload service: "buzzheavier" (default), "pixeldrain", or "gofile"
    /// This determines the order of service attempts when uploading
    #[serde(default)]
    pub preferred_upload_service: PreferredUploadService,
    /// Pixeldrain API key for file uploads (optional)
    #[serde(default)]
    pub pixeldrain_api_key: Option<String>,
    /// Buzzheavier account ID for file uploads (optional)
    #[serde(default)]
    pub buzzheavier_account_id: Option<String>,
    /// Buzzheavier parent folder ID (optional)
    #[serde(default)]
    pub buzzheavier_parent_id: Option<String>,
    /// Gofile token for file uploads (optional)
    #[serde(default)]
    pub gofile_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WidevineConfig {
    #[serde(default = "default_client_id_path")]
    pub client_id_path: PathBuf,
    #[serde(default = "default_private_key_path")]
    pub private_key_path: PathBuf,
}

fn default_locale() -> String {
    "en-US".to_string()
}

fn default_audio_preferences() -> Vec<String> {
    vec!["ja-JP".to_string(), "en-US".to_string()]
}

fn default_temp_dir() -> PathBuf {
    PathBuf::from("./temp")
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("./downloads")
}

fn default_concurrent_segments() -> usize {
    8
}

fn default_client_id_path() -> PathBuf {
    PathBuf::from("src/device/client_id.bin")
}

fn default_private_key_path() -> PathBuf {
    PathBuf::from("src/device/private_key.pem")
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            Error::Config(format!("Failed to read config file '{}': {}", path, e))
        })?;

        let mut config: Config = toml::from_str(&content)?;
        config.validate()?;

        // Convert relative paths to absolute paths
        config.download.temp_dir = Self::resolve_path(&config.download.temp_dir);
        config.download.output_dir = Self::resolve_path(&config.download.output_dir);

        Ok(config)
    }

    /// Resolve a path to absolute, normalizing for the current OS
    fn resolve_path(path: &PathBuf) -> PathBuf {
        if path.is_absolute() {
            path.clone()
        } else {
            // Get current directory and join with the relative path
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let joined = cwd.join(path);

            // On Windows, normalize the path by converting forward slashes
            #[cfg(windows)]
            {
                PathBuf::from(joined.to_string_lossy().replace("/", "\\"))
            }
            #[cfg(not(windows))]
            {
                joined
            }
        }
    }

    pub fn load_or_default() -> Result<Self> {
        let config_paths = ["config.toml", "Config.toml", "settings.toml"];

        for path in &config_paths {
            if std::path::Path::new(path).exists() {
                return Self::load(path);
            }
        }

        Err(Error::Config(
            "No config file found. Create config.toml with required settings.".to_string(),
        ))
    }

    fn validate(&self) -> Result<()> {
        if self.telegram.bot_token.is_empty() {
            return Err(Error::MissingConfig("telegram.bot_token".to_string()));
        }

        if self.crunchyroll.email.is_empty() {
            return Err(Error::MissingConfig("crunchyroll.email".to_string()));
        }

        if self.crunchyroll.password.is_empty() {
            return Err(Error::MissingConfig("crunchyroll.password".to_string()));
        }

        if !self.widevine.client_id_path.exists() {
            return Err(Error::DeviceCredentials(format!(
                "Không tìm thấy file Client ID: {}",
                self.widevine.client_id_path.display()
            )));
        }

        if !self.widevine.private_key_path.exists() {
            return Err(Error::DeviceCredentials(format!(
                "Không tìm thấy file Private Key: {}",
                self.widevine.private_key_path.display()
            )));
        }

        Ok(())
    }
}

impl TelegramConfig {
    /// Check if user is an owner (defined in config.toml)
    pub fn is_owner(&self, user_id: i64) -> bool {
        self.owner_users.iter().any(|o| o.id == user_id)
    }

    /// Check if owner list is empty (no access control configured)
    pub fn has_owners(&self) -> bool {
        !self.owner_users.is_empty()
    }

    /// Get list of owner IDs only
    pub fn owner_ids(&self) -> Vec<i64> {
        self.owner_users.iter().map(|o| o.id).collect()
    }

    /// Format owners for display (usernames if available)
    pub fn format_owners(&self) -> String {
        if self.owner_users.is_empty() {
            return "Không có".to_string();
        }
        self.owner_users
            .iter()
            .map(|o| o.display())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
