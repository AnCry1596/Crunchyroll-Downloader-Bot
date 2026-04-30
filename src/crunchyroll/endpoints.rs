/// Crunchyroll API endpoints and constants

// Base URLs
pub const API_BASE: &str = "https://www.crunchyroll.com";
pub const BETA_API_BASE: &str = "https://beta-api.crunchyroll.com";
pub const PLAYBACK_BASE: &str = "https://cr-play-service.prd.crunchyrollsvc.com/v3";
pub const PLAYBACK_WEB_BASE: &str = "https://www.crunchyroll.com/playback/v2";

// Auth endpoint (on www.crunchyroll.com)
pub const AUTH_TOKEN: &str = "/auth/v1/token";

// Content endpoints (on beta-api.crunchyroll.com)
pub const CMS_BASE: &str = "/content/v2/cms";
pub const DISCOVER_BASE: &str = "/content/v2/discover";

// License endpoint
pub const LICENSE_WIDEVINE: &str = "https://cr-license-proxy.prd.crunchyrollsvc.com/v1/license/widevine";

// Android TV device credentials
pub const BASIC_AUTH: &str = "eTJhcnZqYjBoMHJndnRpemxvdnk6SlZMdndkSXBYdnhVLXFJQnZUMU04b1FUcjFxbFFKWDI=";
pub const USER_AGENT: &str = "Crunchyroll/ANDROIDTV/3.59.0_22338 (Android 12; en-US; SHIELD Android TV Build/SR1A.211012.001)";
pub const DEVICE_TYPE: &str = "Android TV";
pub const DEVICE_NAME: &str = "Android TV";

// Alternative mobile credentials (fallback)
pub const BASIC_AUTH_MOBILE: &str = "cGQ2dXczZGZ5aHpnaHMwd3hhZTM6NXJ5SjJFQXR3TFc0UklIOEozaWk1anVqbnZrRWRfTkY=";
pub const USER_AGENT_MOBILE: &str = "Crunchyroll/3.95.2 Android/16 okhttp/4.12.0";

// Playback endpoints
pub const PLAYBACK_ANDROID_TV: &str = "tv/android_tv";
pub const PLAYBACK_FIREFOX: &str = "web/firefox";
pub const PLAYBACK_WEB_CHROME: &str = "web/chrome";

/// Build full API URL (auth - on beta-api.crunchyroll.com)
pub fn api_url(path: &str) -> String {
    format!("{}{}", BETA_API_BASE, path)
}

/// Build CMS URL (content - on beta-api.crunchyroll.com)
pub fn cms_url(path: &str) -> String {
    format!("{}{}{}", BETA_API_BASE, CMS_BASE, path)
}

/// Build discover URL (content - on beta-api.crunchyroll.com)
pub fn discover_url(path: &str) -> String {
    format!("{}{}{}", BETA_API_BASE, DISCOVER_BASE, path)
}

/// Build playback URL for an episode (EVS3 format - cr-play-service)
pub fn playback_url(media_id: &str, endpoint: &str) -> String {
    format!("{}/{}/{}/play", PLAYBACK_BASE, media_id, endpoint)
}

/// Build web playback URL for an episode (standard DASH format)
pub fn playback_web_url(media_id: &str) -> String {
    format!("{}/{}", PLAYBACK_WEB_BASE, media_id)
}

/// Build series URL
pub fn series_url(series_id: &str) -> String {
    cms_url(&format!("/series/{}", series_id))
}

/// Build seasons URL for a series
pub fn seasons_url(series_id: &str) -> String {
    cms_url(&format!("/series/{}/seasons", series_id))
}

/// Build episodes URL for a season
pub fn episodes_url(season_id: &str) -> String {
    cms_url(&format!("/seasons/{}/episodes", season_id))
}

/// Build episode URL
pub fn episode_url(episode_id: &str) -> String {
    cms_url(&format!("/episodes/{}", episode_id))
}

/// Build movie listing URL
pub fn movie_listing_url(movie_listing_id: &str) -> String {
    cms_url(&format!("/movie_listings/{}", movie_listing_id))
}

/// Build movies URL for a movie listing
pub fn movies_url(movie_listing_id: &str) -> String {
    cms_url(&format!("/movie_listings/{}/movies", movie_listing_id))
}

/// Build movie URL
pub fn movie_url(movie_id: &str) -> String {
    cms_url(&format!("/movies/{}", movie_id))
}

/// Build search URL
pub fn search_url() -> String {
    discover_url("/search")
}
