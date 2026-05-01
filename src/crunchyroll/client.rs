use crate::config::{CrunchyrollConfig, ProxyConfig};
use crate::crunchyroll::auth::AuthManager;
use crate::crunchyroll::endpoints::{
    episode_url, episodes_url, movie_listing_url, movie_url, movies_url, playback_url,
    playback_web_url, search_url, seasons_url, series_url, PLAYBACK_ANDROID_TV, PLAYBACK_FIREFOX,
    USER_AGENT,
};
use crate::crunchyroll::types::{
    ApiResponse, Episode, Movie, MovieListing, SearchItem, SearchResponse, Season, Series,
    StreamResponse,
};
use crate::error::{Error, Result};
use crate::proxy::ProxyManager;
use std::sync::Arc;
use wreq::redirect::Policy;

/// Helper function to create retry policy
fn create_retry_policy() -> wreq::retry::Policy {
    wreq::retry::Policy::default()
        .max_retries_per_request(3)
        .max_extra_load(0.5)
        .classify_fn(|req_rep| {
            // Retry on any error (connection errors, SSL errors, broken pipes, etc.)
            if req_rep.error().is_some() {
                return req_rep.retryable();
            }
            // Retry on server errors (5xx) and 403
            if let Some(status) = req_rep.status() {
                if status.is_server_error() || status == 403 {
                    return req_rep.retryable();
                }
            }
            // Otherwise, consider it successful
            req_rep.success()
        })
}

pub struct CrunchyrollClient {
    /// HTTP client for direct requests (widevine, downloads) - NO proxy
    http_direct: wreq::Client,
    /// HTTP client for SEA proxy (playback first attempt)
    http_sea: Option<wreq::Client>,
    /// HTTP client for US proxy (search, seasons, playback fallback)
    http_us: Option<wreq::Client>,
    /// Auth manager for direct requests (widevine, downloads)
    auth_direct: Arc<AuthManager>,
    /// Auth manager for SEA proxy requests (playback)
    auth_sea: Option<Arc<AuthManager>>,
    /// Auth manager for US proxy requests (search, seasons)
    auth_us: Option<Arc<AuthManager>>,
    locale: String,
    #[allow(dead_code)]
    preferred_audio: Vec<String>,
    proxy_manager: Arc<ProxyManager>,
}

impl CrunchyrollClient {
    pub fn new(config: &CrunchyrollConfig, proxy_config: &ProxyConfig) -> Result<Self> {
        // Create proxy manager
        let proxy_manager = Arc::new(ProxyManager::new(proxy_config.clone()));

        // Direct HTTP client (uses main_proxy if configured, otherwise no proxy)
        // Used for login, widevine, downloads
        let http_direct = {
            let mut builder = wreq::Client::builder()
                .user_agent(USER_AGENT)
                .cookie_store(true)
                .brotli(true)
                .zstd(true)
                .gzip(true)
                .deflate(true)
                .redirect(Policy::limited(10))
                .retry(create_retry_policy());

            // If main_proxy is configured, use it for all "direct" connections
            if let Some(ref main_proxy) = proxy_config.main_proxy {
                if !main_proxy.is_empty() {
                    tracing::info!("Configuring main proxy for all traffic: {}", main_proxy);
                    let proxy = ProxyManager::parse_proxy(main_proxy)?;
                    builder = builder.proxy(proxy);
                }
            }

            builder.build().map_err(|e| Error::Network(e.to_string()))?
        };

        // SEA proxy client (for playback first attempt)
        let http_sea = if let Some(ref sea_proxy) = proxy_config.sea_proxy {
            if !sea_proxy.is_empty() {
                tracing::info!("Configuring SEA proxy client: {}", sea_proxy);
                let proxy = ProxyManager::parse_proxy(sea_proxy)?;
                Some(
                    wreq::Client::builder()
                        .user_agent(USER_AGENT)
                        .cookie_store(true)
                        .brotli(true)
                        .zstd(true)
                        .gzip(true)
                        .deflate(true)
                        .redirect(Policy::limited(10))
                        .retry(create_retry_policy())
                        // .emulation(Emulation::Chrome143)
                        .proxy(proxy)
                        .build()
                        .map_err(|e| Error::Network(e.to_string()))?,
                )
            } else {
                None
            }
        } else {
            None
        };

        // US proxy client (for search, seasons, playback fallback)
        let http_us = if let Some(ref us_proxy) = proxy_config.us_proxy {
            if !us_proxy.is_empty() {
                tracing::info!("Configuring US proxy client: {}", us_proxy);
                let proxy = ProxyManager::parse_proxy(us_proxy)?;
                Some(
                    wreq::Client::builder()
                        .user_agent(USER_AGENT)
                        .cookie_store(true)
                        .brotli(true)
                        .zstd(true)
                        .gzip(true)
                        .deflate(true)
                        .redirect(Policy::limited(10))
                        .retry(create_retry_policy())
                        // .emulation(Emulation::Chrome143)
                        .proxy(proxy)
                        .build()
                        .map_err(|e| Error::Network(e.to_string()))?,
                )
            } else {
                None
            }
        } else {
            None
        };

        // Auth manager for direct requests (widevine, downloads)
        let auth_direct = Arc::new(AuthManager::new(
            http_direct.clone(),
            config.email.clone(),
            config.password.clone(),
        ));

        // Auth manager for SEA proxy requests (playback)
        let auth_sea = if let Some(ref sea_client) = http_sea {
            tracing::info!("Creating separate SEA auth session for SEA region content");
            Some(Arc::new(AuthManager::new(
                sea_client.clone(),
                config.email.clone(),
                config.password.clone(),
            )))
        } else {
            None
        };

        // Auth manager for US proxy requests (search, seasons)
        let auth_us = if let Some(ref us_client) = http_us {
            tracing::info!("Creating separate US auth session for US region content");
            Some(Arc::new(AuthManager::new(
                us_client.clone(),
                config.email.clone(),
                config.password.clone(),
            )))
        } else {
            None
        };

        Ok(Self {
            http_direct,
            http_sea,
            http_us,
            auth_direct,
            auth_sea,
            auth_us,
            locale: config.locale.clone(),
            preferred_audio: config.preferred_audio.clone(),
            proxy_manager,
        })
    }

    /// Initialize proxy - detect geo location
    pub async fn init_proxy(&self) -> Result<()> {
        if let Err(e) = self.proxy_manager.detect_location().await {
            tracing::warn!("Failed to detect geo location: {}. Proxies will still be used if configured.", e);
        }
        Ok(())
    }

    /// Check if US proxy is available
    pub fn has_us_proxy(&self) -> bool {
        self.http_us.is_some()
    }

    /// Check if SEA proxy is available
    pub fn has_sea_proxy(&self) -> bool {
        self.http_sea.is_some()
    }

    /// Get the proxy manager
    pub fn proxy_manager(&self) -> &Arc<ProxyManager> {
        &self.proxy_manager
    }

    /// Get HTTP client for US requests (search, seasons - NOT playback)
    /// These API endpoints accept tokens from any IP
    /// So we use direct connection to save proxy bandwidth
    fn http_for_us(&self) -> &wreq::Client {
        &self.http_direct
    }

    /// Get HTTP client for SEA playback requests
    /// Playback API is IP-locked - must use same IP that obtained the token
 //   async fn http_for_sea_playback(&self) -> &wreq::Client {
        // If we're already in SEA, use direct connection
  //      if self.proxy_manager.is_in_sea().await {
  //          return &self.http_direct;
  //      }
        // Otherwise use SEA proxy if available
  //      if let Some(ref sea) = self.http_sea {
  //          sea
 //       } else {
    //        &self.http_direct
   //     }
  //  }

    /// Get HTTP client for US playback requests
    /// Playback API is IP-locked - must use same IP that obtained the token
//    async fn http_for_us_playback(&self) -> &wreq::Client {
        // If we're already in US, use direct connection
 //       if self.proxy_manager.is_in_us().await {
 //           return &self.http_direct;
 //       }
        // Otherwise use US proxy if available
 //       if let Some(ref us) = self.http_us {
 //           us
  //      } else {
  //          &self.http_direct
  //      }
 //   }

    /// Login to Crunchyroll (logs in through proxies only when needed)
    pub async fn login(&self) -> Result<()> {
        // Login with direct connection (always needed for widevine/downloads)
        tracing::info!("Logging in with direct connection...");
        self.auth_direct.login().await?;
        tracing::info!("Direct login successful");

        // Check hosting location
        let is_in_sea = self.proxy_manager.is_in_sea().await;
        let is_in_us = self.proxy_manager.is_in_us().await;

        // If hosting is in SEA, direct auth already has SEA region - no need for SEA proxy login
        // Only login through SEA proxy if we're NOT in SEA
        if !is_in_sea {
            if let Some(ref auth_sea) = self.auth_sea {
                tracing::info!("Logging in through SEA proxy for SEA region content...");
                if let Err(e) = auth_sea.login().await {
                    tracing::warn!("SEA proxy login failed: {}. SEA content may be limited.", e);
                } else {
                    tracing::info!("SEA proxy login successful");
                }
            }
        } else {
            tracing::info!("Hosting in SEA - using direct auth for SEA content (no separate SEA login needed)");
        }

        // If hosting is in US, direct auth already has US region - no need for US proxy login
        // Only login through US proxy if we're NOT in US
        if !is_in_us {
            if let Some(ref auth_us) = self.auth_us {
                tracing::info!("Logging in through US proxy for US region content...");
                if let Err(e) = auth_us.login().await {
                    tracing::warn!("US proxy login failed: {}. US content may be limited.", e);
                } else {
                    tracing::info!("US proxy login successful");
                }
            }
        } else {
            tracing::info!("Hosting in US - using direct auth for US content (no separate US login needed)");
        }

        Ok(())
    }

    /// Check if authenticated
    pub async fn is_authenticated(&self) -> bool {
        self.auth_direct.is_authenticated().await
    }

    /// Get token for direct requests (widevine, downloads)
    async fn get_token_direct(&self) -> Result<String> {
        self.auth_direct.ensure_valid_token().await
    }

    /// Get token for SEA requests (playback)
    /// Uses SEA auth if available and not in SEA, falls back to direct auth
    async fn get_token_for_sea(&self) -> Result<String> {
        // If we have SEA auth and we're not already in SEA, use it
        if let Some(ref auth_sea) = self.auth_sea {
            if !self.proxy_manager.is_in_sea().await {
                return auth_sea.ensure_valid_token().await;
            }
        }
        // Fall back to direct auth (hosting is in SEA or no SEA proxy)
        self.auth_direct.ensure_valid_token().await
    }

    /// Get token for US requests (search, seasons)
    /// Uses US auth if available and not in US, falls back to direct auth
    async fn get_token_for_us(&self) -> Result<String> {
        // If we have US auth and we're not in US, use it
        if let Some(ref auth_us) = self.auth_us {
            if !self.proxy_manager.is_in_us().await {
                return auth_us.ensure_valid_token().await;
            }
        }
        // Fall back to direct auth (hosting is in US or no US proxy)
        self.auth_direct.ensure_valid_token().await
    }

    /// Force refresh token for direct auth (used on 403 errors)
    pub async fn force_refresh_direct(&self) -> Result<String> {
        self.auth_direct.force_refresh().await
    }

    /// Force refresh token for SEA auth (used on 403 errors)
  //  async fn force_refresh_sea(&self) -> Result<String> {
  //      if let Some(ref auth_sea) = self.auth_sea {
 //           if !self.proxy_manager.is_in_sea().await {
 //               return auth_sea.force_refresh().await;
  //          }
  //      }
  //      self.auth_direct.force_refresh().await
  //  }

    /// Force refresh token for US auth (used on 403 errors)
    async fn force_refresh_us(&self) -> Result<String> {
        if let Some(ref auth_us) = self.auth_us {
            if !self.proxy_manager.is_in_us().await {
                return auth_us.force_refresh().await;
            }
        }
        self.auth_direct.force_refresh().await
    }

    /// Search for anime/series (uses US proxy, or direct if hosting in US)
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchItem>> {
        let mut token = self.get_token_for_us().await?;
        let client = self.http_for_us();

        tracing::debug!("Searching: {}", query);

        // Try up to 2 times (initial + 1 retry after token refresh)
        for attempt in 0..2 {
            let response = client
                .get(search_url())
                .bearer_auth(&token)
                .query(&[
                    ("q", query),
                    ("n", &limit.to_string()),
                    ("type", "series,movie_listing"),
                    ("locale", &self.locale),
                ])
                .send()
                .await?;

            if response.status().as_u16() == 403 && attempt == 0 {
                tracing::warn!("Got 403 on search, refreshing token and retrying...");
                token = self.force_refresh_us().await?;
                continue;
            }

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(Error::Api {
                    code: status.to_string(),
                    message: body,
                });
            }

            let search_response: SearchResponse = response.json().await?;

            let items: Vec<SearchItem> = search_response
                .data
                .into_iter()
                .flat_map(|r| r.items)
                .collect();

            return Ok(items);
        }

        Err(Error::Api {
            code: "403".to_string(),
            message: "Access denied after token refresh".to_string(),
        })
    }

    /// Get series details (uses US proxy, or direct if hosting in US)
    pub async fn get_series(&self, series_id: &str) -> Result<Series> {
        let mut token = self.get_token_for_us().await?;
        let client = self.http_for_us();

        for attempt in 0..2 {
            let response = client
                .get(series_url(series_id))
                .bearer_auth(&token)
                .query(&[
                    ("preferred_audio_language", "ja-JP"),
                    ("locale", &self.locale),
                ])
                .send()
                .await?;

            if response.status().as_u16() == 403 && attempt == 0 {
                tracing::warn!("Got 403 on get_series, refreshing token and retrying...");
                token = self.force_refresh_us().await?;
                continue;
            }

            if !response.status().is_success() {
                if response.status().as_u16() == 404 {
                    return Err(Error::NotFound(format!("Series {} not found", series_id)));
                }
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(Error::Api {
                    code: status.to_string(),
                    message: body,
                });
            }

            let api_response: ApiResponse<Series> = response.json().await?;
            return api_response
                .data
                .into_iter()
                .next()
                .ok_or_else(|| Error::NotFound(format!("Series {} not found", series_id)));
        }

        Err(Error::Api {
            code: "403".to_string(),
            message: "Access denied after token refresh".to_string(),
        })
    }

    /// Get seasons for a series (uses US proxy, or direct if hosting in US)
    pub async fn get_seasons(&self, series_id: &str) -> Result<Vec<Season>> {
        let mut token = self.get_token_for_us().await?;
        let client = self.http_for_us();

        tracing::info!("Fetching seasons for series: {}", series_id);

        for attempt in 0..2 {
            let response = match client
                .get(seasons_url(series_id))
                .bearer_auth(&token)
                .query(&[
                    ("preferred_audio_language", "ja-JP"),
                    ("locale", &self.locale),
                ])
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::error!("Failed to fetch seasons (proxy issue?): {}", e);
                    return Err(e.into());
                }
            };

            tracing::info!("Seasons response status: {}", response.status());

            if response.status().as_u16() == 403 && attempt == 0 {
                tracing::warn!("Got 403 on get_seasons, refreshing token and retrying...");
                token = self.force_refresh_us().await?;
                continue;
            }

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                tracing::error!("Seasons API error: {} - {}", status, &body[..body.len().min(500)]);
                return Err(Error::Api {
                    code: status.to_string(),
                    message: body,
                });
            }

            let body_text = response.text().await.unwrap_or_default();
            tracing::debug!("Seasons response body length: {} bytes", body_text.len());

            let api_response: ApiResponse<Season> = serde_json::from_str(&body_text).map_err(|e| {
                tracing::error!("Failed to parse seasons response: {}", e);
                Error::Api {
                    code: "parse_error".to_string(),
                    message: format!("Failed to parse seasons: {}", e),
                }
            })?;

            tracing::info!("Found {} seasons for series {}", api_response.data.len(), series_id);
            return Ok(api_response.data);
        }

        Err(Error::Api {
            code: "403".to_string(),
            message: "Access denied after token refresh".to_string(),
        })
    }

    /// Get episodes for a season (uses US proxy, or direct if hosting in US)
    pub async fn get_episodes(&self, season_id: &str) -> Result<Vec<Episode>> {
        let mut token = self.get_token_for_us().await?;
        let client = self.http_for_us();

        for attempt in 0..2 {
            let response = client
                .get(episodes_url(season_id))
                .bearer_auth(&token)
                .query(&[
                    ("preferred_audio_language", "ja-JP"),
                    ("locale", &self.locale),
                ])
                .send()
                .await?;

            if response.status().as_u16() == 403 && attempt == 0 {
                tracing::warn!("Got 403 on get_episodes, refreshing token and retrying...");
                token = self.force_refresh_us().await?;
                continue;
            }

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(Error::Api {
                    code: status.to_string(),
                    message: body,
                });
            }

            let api_response: ApiResponse<Episode> = response.json().await?;
            return Ok(api_response.data);
        }

        Err(Error::Api {
            code: "403".to_string(),
            message: "Access denied after token refresh".to_string(),
        })
    }

    /// Get episode details (uses US proxy, or direct if hosting in US)
    pub async fn get_episode(&self, episode_id: &str) -> Result<Episode> {
        let mut token = self.get_token_for_us().await?;
        let client = self.http_for_us();

        for attempt in 0..2 {
            let response = client
                .get(episode_url(episode_id))
                .bearer_auth(&token)
                .query(&[
                    ("preferred_audio_language", "ja-JP"),
                    ("locale", &self.locale),
                ])
                .send()
                .await?;

            if response.status().as_u16() == 403 && attempt == 0 {
                tracing::warn!("Got 403 on get_episode, refreshing token and retrying...");
                token = self.force_refresh_us().await?;
                continue;
            }

            if !response.status().is_success() {
                if response.status().as_u16() == 404 {
                    return Err(Error::NotFound(format!("Episode {} not found", episode_id)));
                }
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(Error::Api {
                    code: status.to_string(),
                    message: body,
                });
            }

            let api_response: ApiResponse<Episode> = response.json().await?;
            return api_response
                .data
                .into_iter()
                .next()
                .ok_or_else(|| Error::NotFound(format!("Episode {} not found", episode_id)));
        }

        Err(Error::Api {
            code: "403".to_string(),
            message: "Access denied after token refresh".to_string(),
        })
    }

    /// Get playback/stream info for an episode
    ///
    /// Strategy for optimal download speed:
    /// 1. Get stream URL (video/audio) from DIRECT IP - nearest CDN for best download speed
    /// 2. Get subtitles from SEA token - more subtitle options available
    /// 3. Fallback to US token for subtitles if SEA doesn't have them
    ///
    /// On 403 errors, refreshes token and retries
    pub async fn get_playback(&self, media_id: &str) -> Result<StreamResponse> {
        // Step 1: Get stream URL from DIRECT connection (nearest CDN for best speed)
        let direct_client = &self.http_direct;
        let mut direct_token = self.get_token_direct().await?;
        tracing::info!("Getting stream URL with DIRECT connection for best download speed");

        let mut stream_response: Option<StreamResponse> = None;

        // Try direct connection first for stream URL
        for attempt in 0..2 {
            match self.get_playback_with_client(direct_client, &direct_token, media_id).await {
                Ok(response) => {
                    stream_response = Some(response);
                    break;
                }
                Err(Error::Api { ref code, .. }) if code == "403" && attempt == 0 => {
                    tracing::warn!("Got 403 on direct playback, refreshing token and retrying...");
                    direct_token = self.force_refresh_direct().await?;
                    continue;
                }
                Err(e) => {
                    tracing::debug!("Direct connection playback failed: {}", e);
                    break;
                }
            }
        }

        // // If direct failed, try SEA token (still with direct connection for speed)
        // if stream_response.is_none() {
        //     tracing::info!("Direct token failed, trying SEA token with direct connection");
        //     let mut sea_token = self.get_token_for_sea().await?;

        //     for attempt in 0..2 {
        //         match self.get_playback_with_client(direct_client, &sea_token, media_id).await {
        //             Ok(response) => {
        //                 stream_response = Some(response);
        //                 break;
        //             }
        //             Err(Error::Api { ref code, .. }) if code == "403" && attempt == 0 => {
        //                 tracing::warn!("Got 403 on SEA token playback, refreshing and retrying...");
        //                 sea_token = self.force_refresh_sea().await?;
        //                 continue;
        //             }
        //             Err(e) => {
        //                 tracing::debug!("SEA token playback failed: {}", e);
        //                 break;
        //             }
        //         }
        //     }
        // }

        // If still no stream, try US token as last resort
        if stream_response.is_none() {
            tracing::info!("SEA token failed, trying US token with direct connection");
            let mut us_token = self.get_token_for_us().await?;

            for attempt in 0..2 {
                match self.get_playback_with_client(direct_client, &us_token, media_id).await {
                    Ok(response) => {
                        stream_response = Some(response);
                        break;
                    }
                    Err(Error::Api { ref code, .. }) if code == "403" && attempt == 0 => {
                        tracing::warn!("Got 403 on US token playback, refreshing and retrying...");
                        us_token = self.force_refresh_us().await?;
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        let mut response = stream_response.ok_or_else(|| Error::Api {
            code: "playback_failed".to_string(),
            message: "Failed to get playback with any token".to_string(),
        })?;

        // Step 2: Always fetch SEA + US playback to merge versions and subtitles from all regions.
        // SEA has vi-VN audio; US has en-US and others. Merge versions (audio list) and subtitles.
        let merge_from_region = |response: &mut StreamResponse, other: StreamResponse, label: &str| {
            let added_versions = if let Some(other_versions) = other.versions {
                let existing_guids: std::collections::HashSet<_> = response.versions
                    .as_ref()
                    .map(|v| v.iter().filter_map(|x| x.guid.clone()).collect())
                    .unwrap_or_default();
                let new_versions: Vec<_> = other_versions.into_iter()
                    .filter(|v| v.guid.as_ref().map(|g| !existing_guids.contains(g)).unwrap_or(false))
                    .collect();
                let count = new_versions.len();
                if !new_versions.is_empty() {
                    response.versions.get_or_insert_with(Vec::new).extend(new_versions);
                }
                count
            } else { 0 };

            let mut added_subs = 0;
            for (locale, track) in other.subtitles {
                if response.subtitles.insert(locale, track).is_none() { added_subs += 1; }
            }
            for (locale, track) in other.captions {
                if response.captions.insert(locale, track).is_none() { added_subs += 1; }
            }
            tracing::info!("{}: merged {} new audio versions, {} new subtitles", label, added_versions, added_subs);
        };

        if let Ok(sea_token) = self.get_token_for_sea().await {
            match self.get_playback_with_client(direct_client, &sea_token, media_id).await {
                Ok(sea_response) => merge_from_region(&mut response, sea_response, "SEA"),
                Err(e) => tracing::debug!("SEA playback fetch failed: {}", e),
            }
        }

        if let Ok(us_token) = self.get_token_for_us().await {
            match self.get_playback_with_client(direct_client, &us_token, media_id).await {
                Ok(us_response) => merge_from_region(&mut response, us_response, "US"),
                Err(e) => tracing::debug!("US playback fetch failed: {}", e),
            }
        }

        let final_subtitle_count = response.subtitles.len() + response.captions.len();
        let final_version_count = response.versions.as_ref().map(|v| v.len()).unwrap_or(0);
        tracing::info!("Final playback response: stream URL present={}, subtitles={}, audio versions={}",
            response.url.is_some(), final_subtitle_count, final_version_count);

        Ok(response)
    }

    /// Internal playback implementation with specified HTTP client and token
    ///
    /// Tries endpoints in order:
    /// 1. cr-play-service Android TV endpoint (best quality, may not be IP-locked)
    /// 2. Web playback endpoint (standard DASH)
    /// 3. Firefox endpoint as last resort
    async fn get_playback_with_client(&self, client: &wreq::Client, token: &str, media_id: &str) -> Result<StreamResponse> {
        // Try Android TV endpoint FIRST (cr-play-service)
        // This endpoint is separate from the main API and may not be IP-locked
        let url = playback_url(media_id, PLAYBACK_ANDROID_TV);
        tracing::info!("Trying cr-play-service Android TV endpoint: {}", url);

        let response = client
            .get(&url)
            .bearer_auth(&token)
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;

        if response.status().is_success() {
            let body_text = response.text().await.map_err(|e| {
                Error::Api {
                    code: "read_error".to_string(),
                    message: format!("Failed to read playback response: {}", e),
                }
            })?;

            // Save to debug file
            if let Err(e) = tokio::fs::write("temp/debug_playback_androidtv.json", &body_text).await {
                tracing::warn!("Failed to save debug playback response: {}", e);
            } else {
                tracing::debug!("Saved Android TV playback response to temp/debug_playback_androidtv.json");
            }

            tracing::info!("cr-play-service Android TV endpoint successful");
            return serde_json::from_str(&body_text).map_err(|e| {
                Error::Api {
                    code: "parse_error".to_string(),
                    message: format!("Failed to parse playback response: {}", e),
                }
            });
        }

        // Check for specific errors from Android TV endpoint
        let atv_status = response.status();
        let atv_body = response.text().await.unwrap_or_default();
        tracing::debug!("Android TV endpoint failed ({}): {}", atv_status, &atv_body[..atv_body.len().min(200)]);

        // Return 403 immediately so caller can refresh token and retry
        if atv_status.as_u16() == 403 {
            return Err(Error::Api {
                code: "403".to_string(),
                message: atv_body,
            });
        }

        if atv_body.contains("TOO_MANY_ACTIVE_STREAMS") {
            return Err(Error::TooManyStreams);
        }

        if atv_body.contains("NOT_PREMIUM") || atv_body.contains("premium") {
            return Err(Error::PremiumRequired);
        }

        // Try web endpoint as fallback - returns proper DASH manifests with mimeType attributes
        let url = playback_web_url(media_id);
        tracing::debug!("Trying web playback endpoint: {}", url);

        let response = client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await?;

        if response.status().is_success() {
            let body_text = response.text().await.map_err(|e| {
                Error::Api {
                    code: "read_error".to_string(),
                    message: format!("Failed to read playback response: {}", e),
                }
            })?;

            // Save to debug file
            if let Err(e) = tokio::fs::write("temp/debug_playback_web.json", &body_text).await {
                tracing::warn!("Failed to save debug playback response: {}", e);
            } else {
                tracing::debug!("Saved web playback response to temp/debug_playback_web.json");
            }

            tracing::info!("Web playback endpoint successful");
            return serde_json::from_str(&body_text).map_err(|e| {
                Error::Api {
                    code: "parse_error".to_string(),
                    message: format!("Failed to parse playback response: {}", e),
                }
            });
        }

        // Check for specific errors from web endpoint
        let web_status = response.status();
        let web_body = response.text().await.unwrap_or_default();
        tracing::debug!("Web endpoint failed ({}): {}", web_status, &web_body[..web_body.len().min(200)]);

        // Return 403 immediately so caller can refresh token and retry
        if web_status.as_u16() == 403 {
            return Err(Error::Api {
                code: "403".to_string(),
                message: web_body,
            });
        }

        if web_body.contains("TOO_MANY_ACTIVE_STREAMS") {
            return Err(Error::TooManyStreams);
        }

        if web_body.contains("NOT_PREMIUM") || web_body.contains("premium") {
            return Err(Error::PremiumRequired);
        }

        // Try Firefox as last resort
        tracing::debug!("Trying Firefox endpoint as last resort");
        let url = playback_url(media_id, PLAYBACK_FIREFOX);

        let response = client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await?;

        if !response.status().is_success() {
            // Return the most informative error (from Android TV attempt)
            return Err(Error::Api {
                code: atv_status.to_string(),
                message: atv_body,
            });
        }

        tracing::info!("Firefox endpoint successful");
        response.json().await.map_err(|e| Error::Api {
            code: "parse_error".to_string(),
            message: format!("Failed to parse playback response: {}", e),
        })
    }

    /// Get the account ID
    pub async fn get_account_id(&self) -> Option<String> {
        self.auth_direct.get_account_id().await
    }

    /// Get the country code
    pub async fn get_country(&self) -> Option<String> {
        self.auth_direct.get_country().await
    }

    /// Get HTTP client for direct requests (e.g., license server, downloads)
    pub fn http(&self) -> &wreq::Client {
        &self.http_direct
    }

    /// Get current access token (direct - for widevine/downloads)
    pub async fn access_token(&self) -> Result<String> {
        self.get_token_direct().await
    }

    /// Get movie listing details (uses US proxy, or direct if hosting in US)
    pub async fn get_movie_listing(&self, movie_listing_id: &str) -> Result<MovieListing> {
        let mut token = self.get_token_for_us().await?;
        let client = self.http_for_us();

        for attempt in 0..2 {
            let response = client
                .get(movie_listing_url(movie_listing_id))
                .bearer_auth(&token)
                .query(&[
                    ("preferred_audio_language", "ja-JP"),
                    ("locale", &self.locale),
                ])
                .send()
                .await?;

            if response.status().as_u16() == 403 && attempt == 0 {
                tracing::warn!("Got 403 on get_movie_listing, refreshing token and retrying...");
                token = self.force_refresh_us().await?;
                continue;
            }

            if !response.status().is_success() {
                if response.status().as_u16() == 404 {
                    return Err(Error::NotFound(format!(
                        "Movie listing {} not found",
                        movie_listing_id
                    )));
                }
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(Error::Api {
                    code: status.to_string(),
                    message: body,
                });
            }

            let api_response: ApiResponse<MovieListing> = response.json().await?;
            return api_response
                .data
                .into_iter()
                .next()
                .ok_or_else(|| Error::NotFound(format!("Movie listing {} not found", movie_listing_id)));
        }

        Err(Error::Api {
            code: "403".to_string(),
            message: "Access denied after token refresh".to_string(),
        })
    }

    /// Get movies for a movie listing (uses US proxy, or direct if hosting in US)
    pub async fn get_movies(&self, movie_listing_id: &str) -> Result<Vec<Movie>> {
        let mut token = self.get_token_for_us().await?;
        let client = self.http_for_us();

        for attempt in 0..2 {
            let response = client
                .get(movies_url(movie_listing_id))
                .bearer_auth(&token)
                .query(&[
                    ("preferred_audio_language", "ja-JP"),
                    ("locale", &self.locale),
                ])
                .send()
                .await?;

            if response.status().as_u16() == 403 && attempt == 0 {
                tracing::warn!("Got 403 on get_movies, refreshing token and retrying...");
                token = self.force_refresh_us().await?;
                continue;
            }

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(Error::Api {
                    code: status.to_string(),
                    message: body,
                });
            }

            let api_response: ApiResponse<Movie> = response.json().await?;
            return Ok(api_response.data);
        }

        Err(Error::Api {
            code: "403".to_string(),
            message: "Access denied after token refresh".to_string(),
        })
    }

    /// Get movie details (uses US proxy, or direct if hosting in US)
    pub async fn get_movie(&self, movie_id: &str) -> Result<Movie> {
        let mut token = self.get_token_for_us().await?;
        let client = self.http_for_us();

        for attempt in 0..2 {
            let response = client
                .get(movie_url(movie_id))
                .bearer_auth(&token)
                .query(&[
                    ("preferred_audio_language", "ja-JP"),
                    ("locale", &self.locale),
                ])
                .send()
                .await?;

            if response.status().as_u16() == 403 && attempt == 0 {
                tracing::warn!("Got 403 on get_movie, refreshing token and retrying...");
                token = self.force_refresh_us().await?;
                continue;
            }

            if !response.status().is_success() {
                if response.status().as_u16() == 404 {
                    return Err(Error::NotFound(format!("Movie {} not found", movie_id)));
                }
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(Error::Api {
                    code: status.to_string(),
                    message: body,
                });
            }

            let api_response: ApiResponse<Movie> = response.json().await?;
            return api_response
                .data
                .into_iter()
                .next()
                .ok_or_else(|| Error::NotFound(format!("Movie {} not found", movie_id)));
        }

        Err(Error::Api {
            code: "403".to_string(),
            message: "Access denied after token refresh".to_string(),
        })
    }
}
