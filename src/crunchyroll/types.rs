use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Authentication Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_in: u64,
    pub scope: String,
    pub country: String,
    pub account_id: String,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AuthSession {
    pub token: TokenResponse,
    pub expires_at: DateTime<Utc>,
    pub device_id: String,
}

impl AuthSession {
    pub fn new(token: TokenResponse, device_id: String) -> Self {
        let expires_at = Utc::now() + chrono::Duration::seconds(token.expires_in as i64);
        Self {
            token,
            expires_at,
            device_id,
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    pub fn needs_refresh(&self) -> bool {
        Utc::now() >= self.expires_at - chrono::Duration::minutes(5)
    }

    pub fn access_token(&self) -> &str {
        &self.token.access_token
    }
}

// ============================================================================
// API Response Wrappers
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct ApiResponse<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub total: Option<u32>,
    #[serde(default)]
    pub meta: Option<ResponseMeta>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseMeta {
    pub total_before_filter: Option<u32>,
    pub total: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub context: Option<Vec<ErrorContext>>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorContext {
    pub code: String,
    pub field: Option<String>,
}

// ============================================================================
// Search Types
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    pub data: Vec<SearchResult>,
    pub total: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    #[serde(rename = "type")]
    pub result_type: String,
    pub count: u32,
    pub items: Vec<SearchItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub title: String,
    #[serde(default)]
    pub slug_title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub images: Option<Images>,
    #[serde(default)]
    pub series_metadata: Option<SeriesMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeriesMetadata {
    pub episode_count: Option<u32>,
    pub season_count: Option<u32>,
    pub is_mature: Option<bool>,
    pub is_subbed: Option<bool>,
    pub is_dubbed: Option<bool>,
}

// ============================================================================
// Content Types
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct Series {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub slug_title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub images: Option<Images>,
    #[serde(default)]
    pub episode_count: Option<u32>,
    #[serde(default)]
    pub season_count: Option<u32>,
    #[serde(default)]
    pub is_mature: Option<bool>,
    #[serde(default)]
    pub maturity_ratings: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Season {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub season_number: Option<u32>,
    #[serde(default)]
    pub season_sequence_number: Option<u32>,
    #[serde(default)]
    pub number_of_episodes: Option<u32>,
    #[serde(default)]
    pub is_dubbed: Option<bool>,
    #[serde(default)]
    pub is_subbed: Option<bool>,
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub audio_locales: Option<Vec<String>>,
    #[serde(default)]
    pub subtitle_locales: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Episode {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub episode: Option<String>,
    #[serde(default)]
    pub episode_number: Option<u32>,
    #[serde(default)]
    pub season_number: Option<u32>,
    #[serde(default)]
    pub sequence_number: Option<f32>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub images: Option<Images>,
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub subtitle_locales: Option<Vec<String>>,
    #[serde(default)]
    pub is_premium_only: Option<bool>,
    #[serde(default)]
    pub streams_link: Option<String>,
    #[serde(default)]
    pub series_id: Option<String>,
    #[serde(default)]
    pub series_title: Option<String>,
    #[serde(default)]
    pub season_id: Option<String>,
    #[serde(default)]
    pub season_title: Option<String>,
    #[serde(default)]
    pub versions: Option<Vec<Version>>,
}

impl Episode {
    pub fn display_number(&self) -> String {
        self.episode
            .clone()
            .or_else(|| self.episode_number.map(|n| n.to_string()))
            .unwrap_or_else(|| "?".to_string())
    }

    pub fn duration_formatted(&self) -> String {
        if let Some(ms) = self.duration_ms {
            let total_seconds = ms / 1000;
            let minutes = total_seconds / 60;
            let seconds = total_seconds % 60;
            format!("{}:{:02}", minutes, seconds)
        } else {
            "?:??".to_string()
        }
    }
}

/// Movie listing (collection of movies, similar to series)
#[derive(Debug, Clone, Deserialize)]
pub struct MovieListing {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub slug_title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub images: Option<Images>,
    #[serde(default)]
    pub movie_release_year: Option<u32>,
    #[serde(default)]
    pub is_mature: Option<bool>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

/// Movie (individual movie, similar to episode)
#[derive(Debug, Clone, Deserialize)]
pub struct Movie {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub images: Option<Images>,
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub subtitle_locales: Option<Vec<String>>,
    #[serde(default)]
    pub is_premium_only: Option<bool>,
    #[serde(default)]
    pub streams_link: Option<String>,
    #[serde(default)]
    pub movie_listing_id: Option<String>,
    #[serde(default)]
    pub movie_listing_title: Option<String>,
}

impl Movie {
    pub fn duration_formatted(&self) -> String {
        if let Some(ms) = self.duration_ms {
            let total_seconds = ms / 1000;
            let hours = total_seconds / 3600;
            let minutes = (total_seconds % 3600) / 60;
            if hours > 0 {
                format!("{}h {}m", hours, minutes)
            } else {
                format!("{}m", minutes)
            }
        } else {
            "?:??".to_string()
        }
    }

    /// Convert Movie to Episode for download compatibility
    pub fn to_episode(&self) -> Episode {
        Episode {
            id: self.id.clone(),
            title: self.title.clone(),
            episode: Some("Movie".to_string()),
            episode_number: Some(1),
            season_number: None,
            sequence_number: None,
            description: self.description.clone(),
            duration_ms: self.duration_ms,
            images: self.images.clone(),
            audio_locale: self.audio_locale.clone(),
            subtitle_locales: self.subtitle_locales.clone(),
            is_premium_only: self.is_premium_only,
            streams_link: self.streams_link.clone(),
            series_id: self.movie_listing_id.clone(),
            series_title: self.movie_listing_title.clone(),
            season_id: None,
            season_title: None,
            versions: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Images {
    #[serde(default)]
    pub poster_tall: Option<Vec<Vec<ImageVariant>>>,
    #[serde(default)]
    pub poster_wide: Option<Vec<Vec<ImageVariant>>>,
    #[serde(default)]
    pub thumbnail: Option<Vec<Vec<ImageVariant>>>,
}

impl Images {
    pub fn get_thumbnail(&self) -> Option<String> {
        self.thumbnail
            .as_ref()
            .and_then(|t| t.first())
            .and_then(|v| v.last())
            .map(|i| i.source.clone())
    }

    pub fn get_poster(&self) -> Option<String> {
        self.poster_tall
            .as_ref()
            .and_then(|t| t.first())
            .and_then(|v| v.last())
            .map(|i| i.source.clone())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageVariant {
    pub width: u32,
    pub height: u32,
    pub source: String,
    #[serde(rename = "type")]
    pub image_type: Option<String>,
}

// ============================================================================
// Playback Types
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct PlaybackResponse {
    #[serde(default)]
    pub media_id: Option<String>,
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub subtitles: HashMap<String, SubtitleTrack>,
    #[serde(default)]
    pub captions: HashMap<String, SubtitleTrack>,
    #[serde(default)]
    pub bifs: Option<Vec<String>>,
    #[serde(default)]
    pub versions: Option<Vec<Version>>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubtitleTrack {
    #[serde(default, alias = "language")]
    pub locale: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Version {
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub guid: Option<String>,
    #[serde(default)]
    pub is_premium_only: Option<bool>,
    #[serde(default)]
    pub media_guid: Option<String>,
    #[serde(default)]
    pub original: Option<bool>,
    #[serde(default)]
    pub season_guid: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
}

/// Information about one audio version's stream, ready for download
#[derive(Debug, Clone)]
pub struct AudioVersionInfo {
    pub audio_locale: String,
    pub guid: String,
    pub stream_url: String,
    pub drm_pssh: Option<String>,
    pub video_token: Option<String>,
    pub content_id: Option<String>,
}

// Extended playback response with DRM info
#[derive(Debug, Clone, Deserialize)]
pub struct StreamResponse {
    #[serde(default)]
    pub media_id: Option<String>,
    #[serde(default)]
    pub audio_locale: Option<String>,
    #[serde(default)]
    pub subtitles: HashMap<String, SubtitleTrack>,
    #[serde(default)]
    pub captions: HashMap<String, SubtitleTrack>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub versions: Option<Vec<Version>>,
    #[serde(default, rename = "assetId")]
    pub asset_id: Option<String>,
    #[serde(default, rename = "playbackType")]
    pub playback_type: Option<String>,
    #[serde(default)]
    pub drm: Option<DrmInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DrmInfo {
    #[serde(rename = "type")]
    pub drm_type: Option<String>,
    #[serde(default)]
    pub key_id: Option<String>,
    #[serde(default)]
    pub pssh: Option<String>,
    #[serde(default)]
    pub license_url: Option<String>,
}

// ============================================================================
// Stream Quality Selection
// ============================================================================

#[derive(Debug, Clone)]
pub struct StreamQuality {
    pub width: u32,
    pub height: u32,
    pub bitrate: u64,
    pub codec: String,
    pub url: String,
}

impl StreamQuality {
    pub fn label(&self) -> String {
        format!("{}p", self.height)
    }

    pub fn bitrate_formatted(&self) -> String {
        if self.bitrate >= 1_000_000 {
            format!("{:.1} Mbps", self.bitrate as f64 / 1_000_000.0)
        } else {
            format!("{} kbps", self.bitrate / 1000)
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamSelection {
    pub video: StreamQuality,
    pub audio_url: Option<String>,
    pub subtitles: Vec<SubtitleTrack>,
    pub drm_info: Option<DrmInfo>,
}
