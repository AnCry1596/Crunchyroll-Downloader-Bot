use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    // HTTP/Network errors
    #[error("HTTP request failed: {0}")]
    Http(#[from] wreq::Error),

    #[error("Network error: {0}")]
    Network(String),

    // Authentication errors
    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Token expired")]
    TokenExpired,

    #[error("Invalid credentials")]
    InvalidCredentials,

    // API errors
    #[error("API error: {message} (code: {code})")]
    Api { code: String, message: String },

    #[error("Content not found: {0}")]
    NotFound(String),

    #[error("Premium content requires subscription")]
    PremiumRequired,

    #[error("Too many active streams")]
    TooManyStreams,

    // DRM errors
    #[error("Widevine error: {0}")]
    Widevine(String),

    #[error("Failed to load device credentials: {0}")]
    DeviceCredentials(String),

    #[error("License request failed: {0}")]
    License(String),

    #[error("No content keys available")]
    NoContentKeys,

    // Download errors
    #[error("Download failed: {0}")]
    Download(String),

    #[error("Manifest parsing failed: {0}")]
    ManifestParse(String),

    #[error("Decryption failed: {0}")]
    Decryption(String),

    #[error("Muxing failed: {0}")]
    Muxing(String),

    #[error("Download cancelled")]
    Cancelled,

    #[error("Stream URL expired (403), needs refresh")]
    StreamUrlExpired {
        /// Number of segments already completed for video
        video_segments_completed: usize,
        /// Number of segments already completed for audio
        audio_segments_completed: usize,
        /// Bytes already downloaded
        bytes_downloaded: u64,
    },

    // Telegram errors
    #[error("Telegram error: {0}")]
    Telegram(String),

    #[error("File upload failed: {0}")]
    Upload(String),

    #[error("File too large for Telegram: {size} bytes")]
    FileTooLarge { size: u64 },

    // Config errors
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Missing configuration: {0}")]
    MissingConfig(String),

    // IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // Serialization errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    // External tool errors
    #[error("External tool error ({tool}): {message}")]
    ExternalTool { tool: String, message: String },

    #[error("mp4decrypt not found. Please install Bento4.")]
    Mp4DecryptNotFound,

    #[error("FFmpeg not found. Please install FFmpeg.")]
    FfmpegNotFound,

    // Database errors
    #[error("Database error: {0}")]
    Database(String),
}

impl Error {
    pub fn api(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Api {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn external_tool(tool: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ExternalTool {
            tool: tool.into(),
            message: message.into(),
        }
    }
}
