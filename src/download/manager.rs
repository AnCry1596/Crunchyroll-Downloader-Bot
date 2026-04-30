use crate::config::DownloadConfig;
use crate::crunchyroll::types::{AudioVersionInfo, Episode, SubtitleTrack};
use crate::database::models::{CachedKey, KeyPair};
use crate::database::Database;
use crate::download::dash::DashParser;
use crate::download::progress::{DownloadPhase, DownloadState, SharedProgress};
use crate::download::segment::{SegmentDownloader, SegmentDownloaderConfig, merge_segments, cleanup_segments};
use crate::drm::decrypt::{Decrypter, Muxer};
use crate::drm::widevine::{ContentKey, KeyType, WidevineCdm};
use crate::error::{Error, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::fs;

/// Download task information
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub id: String,
    pub episode: Episode,
    pub stream_url: String,
    pub drm_pssh: Option<String>,
    pub subtitles: Vec<SubtitleTrack>,
    /// Video token for license request (X-Cr-Video-Token header)
    pub video_token: Option<String>,
    /// Content ID for license request (X-Cr-Content-Id header)
    pub content_id: Option<String>,
    /// Additional audio versions to download (beyond the primary stream's audio)
    pub additional_audio_versions: Vec<AudioVersionInfo>,
    /// Audio locale of the primary stream
    pub primary_audio_locale: Option<String>,
}

/// Download result
#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub path: PathBuf,
    pub size: u64,
    pub filename: String,
    /// Decryption keys used (kid, key) pairs
    pub decryption_keys: Vec<(String, String)>,
    /// Temp directory used for this download (for cleanup)
    pub temp_dir: PathBuf,
    /// Video width in pixels
    pub width: u32,
    /// Video height in pixels
    pub height: u32,
    /// Subtitle locales that were included
    pub subtitle_locales: Vec<String>,
    /// Audio locale (primary)
    pub audio_locale: Option<String>,
    /// All audio locales included in the muxed file
    pub audio_locales: Vec<String>,
}

/// Downloaded audio track with metadata
#[derive(Debug, Clone)]
pub struct DownloadedAudioTrack {
    pub path: PathBuf,
    pub locale: String,
}

/// Downloaded subtitle with metadata
#[derive(Debug, Clone)]
pub struct DownloadedSubtitle {
    pub path: PathBuf,
    pub locale: String,
}

/// Download manager for handling video downloads with custom segment downloader
pub struct DownloadManager {
    http: wreq::Client,
    cdm: Option<WidevineCdm>,
    config: DownloadConfig,
    cancelled: Arc<AtomicBool>,
    /// Path to mp4decrypt executable
    mp4decrypt_path: Option<PathBuf>,
    /// Path to ffmpeg executable
    ffmpeg_path: Option<PathBuf>,
    /// Database for caching keys
    database: Option<Database>,
}

impl DownloadManager {
    pub fn new(http: wreq::Client, cdm: Option<WidevineCdm>, config: DownloadConfig, database: Option<Database>) -> Self {
        // Check for tools in temp/tools directory
        let tools_dir = config.temp_dir.join("tools");
        let mp4decrypt_path = {
            let path = tools_dir.join(if cfg!(windows) { "mp4decrypt.exe" } else { "mp4decrypt" });
            if path.exists() { Some(path) } else { None }
        };
        let ffmpeg_path = {
            let path = tools_dir.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
            if path.exists() { Some(path) } else { None }
        };

        Self {
            http,
            cdm,
            config,
            cancelled: Arc::new(AtomicBool::new(false)),
            mp4decrypt_path,
            ffmpeg_path,
            database,
        }
    }

    /// Set custom tool paths
    pub fn set_tool_paths(&mut self, _n_m3u8dl_re: Option<PathBuf>, mp4decrypt: Option<PathBuf>, ffmpeg: Option<PathBuf>) {
        self.mp4decrypt_path = mp4decrypt;
        self.ffmpeg_path = ffmpeg;
    }

    /// Cancel ongoing download
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Reset cancellation flag
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }

    /// Check if cancelled
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Download an episode using N_m3u8DL-RE
    pub async fn download(
        &self,
        task: &DownloadTask,
        auth_token: &str,
        progress: SharedProgress,
    ) -> Result<DownloadResult> {
        self.download_with_resume(task, auth_token, progress, 0, 0).await
    }

    /// Download an episode using N_m3u8DL-RE with resume support
    pub async fn download_with_resume(
        &self,
        task: &DownloadTask,
        auth_token: &str,
        progress: SharedProgress,
        _video_start_segment: usize,
        _audio_start_segment: usize,
    ) -> Result<DownloadResult> {
        self.reset();

        // Create unique temp folder for this download task
        let task_temp_dir = self.config.temp_dir.join(&task.id);
        tracing::info!("Creating temp directory: {:?}", task_temp_dir);
        tracing::info!("Output directory: {:?}", self.config.output_dir);
        fs::create_dir_all(&task_temp_dir).await?;
        fs::create_dir_all(&self.config.output_dir).await?;

        // Update progress and start timing
        {
            let mut p = progress.write().await;
            p.set_state(DownloadState::InProgress);
            p.set_phase(DownloadPhase::FetchingManifest);
            p.start_timing();
            p.set_episode_info(
                task.episode.series_title.clone(),
                task.episode.season_title.clone(),
                Some(task.episode.title.clone()),
                Some(task.episode.display_number()),
            );
        }

        // Fetch and parse manifest to get PSSH and video info
        tracing::info!("Fetching manifest from: {}", task.stream_url);
        let manifest_content = self.fetch_manifest(&task.stream_url, auth_token).await?;

        if self.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let parser = DashParser::new(&task.stream_url);
        let parsed = parser.parse(&manifest_content)?;

        tracing::info!(
            "Parsed {} video and {} audio representations",
            parsed.video_representations.len(),
            parsed.audio_representations.len()
        );

        // Get best video to determine resolution
        let video_rep = parsed
            .best_video()
            .ok_or_else(|| Error::Download("No video representations found".to_string()))?;

        let video_width = video_rep.width.unwrap_or(1920);
        let video_height = video_rep.height.unwrap_or(1080);

        tracing::info!(
            "Selected video: {}x{} @ {} bps",
            video_width,
            video_height,
            video_rep.bandwidth
        );

        // Estimate file size — prefer manifest duration, then episode metadata, then segment count
        let video_duration_secs = parsed.duration_secs()
            .or_else(|| task.episode.duration_ms.map(|ms| ms as f64 / 1000.0))
            .unwrap_or_else(|| {
                // Last resort: count segments × 6s average segment duration
                video_rep.segments.len() as f64 * 6.0
            });
        let audio_rep = parsed.best_audio();
        let video_est_bytes = (video_rep.bandwidth as f64 / 8.0 * video_duration_secs) as u64;
        let audio_est_bytes = audio_rep
            .map(|a| (a.bandwidth as f64 / 8.0 * video_duration_secs) as u64)
            .unwrap_or(0);
        let estimated_total = video_est_bytes + audio_est_bytes;

        {
            let mut p = progress.write().await;
            p.estimated_file_size = estimated_total;
        }

        // Get decryption keys if DRM protected
        let mut decryption_keys: Vec<(String, String)> = Vec::new();
        if let Some(pssh) = task.drm_pssh.as_ref().or(parsed.pssh.as_ref()) {
            {
                let mut p = progress.write().await;
                p.set_phase(DownloadPhase::FetchingKeys);
            }

            // Check database cache first
            let cached_keys = if let Some(ref db) = self.database {
                match db.get_cached_keys(pssh).await {
                    Ok(Some(cached)) => {
                        tracing::info!("Found {} cached keys for PSSH in database", cached.keys.len());
                        let _ = db.increment_key_use_count(pssh).await;
                        let keys: Vec<ContentKey> = cached.keys.iter().map(|k| ContentKey {
                            kid: k.kid.clone(),
                            key: k.key.clone(),
                            key_type: KeyType::Content,
                        }).collect();
                        Some(keys)
                    }
                    Ok(None) => {
                        tracing::info!("No cached keys found for PSSH");
                        None
                    }
                    Err(e) => {
                        tracing::warn!("Error checking cached keys: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            let keys = if let Some(keys) = cached_keys {
                keys
            } else if let Some(ref cdm) = self.cdm {
                tracing::info!("Fetching decryption keys from license server");
                let keys = cdm
                    .get_content_keys(
                        pssh,
                        None,
                        &self.http,
                        auth_token,
                        task.content_id.as_deref(),
                        task.video_token.as_deref(),
                    )
                    .await?;

                // Save keys to database
                if let Some(ref db) = self.database {
                    let key_pairs: Vec<KeyPair> = keys.iter().map(|k| KeyPair {
                        kid: k.kid.clone(),
                        key: k.key.clone(),
                    }).collect();
                    let cached = CachedKey::new(pssh.clone(), key_pairs)
                        .with_content_id(task.content_id.clone().unwrap_or_default());
                    if let Err(e) = db.save_cached_keys(&cached).await {
                        tracing::warn!("Failed to cache keys to database: {}", e);
                    } else {
                        tracing::info!("Saved {} keys to database for PSSH", keys.len());
                    }
                }

                keys
            } else {
                tracing::warn!("DRM content but no CDM available");
                return Err(Error::Download("DRM content but no CDM available".to_string()));
            };

            // Store keys for result
            decryption_keys = keys.iter().map(|k| (k.kid.clone(), k.key.clone())).collect();
        }

        if self.is_cancelled() {
            return Err(Error::Cancelled);
        }

        // Create segment downloader with config
        let segment_config = SegmentDownloaderConfig {
            max_concurrent: self.config.max_concurrent_segments,
            retry_count: 3,
            retry_delay_ms: 1000,
        };
        let segment_downloader = SegmentDownloader::new_with_cancelled(self.http.clone(), segment_config, self.cancelled.clone());

        // Download video segments
        tracing::info!("Downloading video segments ({} total)", video_rep.segments.len());
        let video_segments_dir = task_temp_dir.join("video_segments");
        let video_segments = segment_downloader
            .download_representation(video_rep, &video_segments_dir, progress.clone(), DownloadPhase::DownloadingVideo)
            .await?;

        if self.is_cancelled() {
            cleanup_segments(&video_segments).await;
            return Err(Error::Cancelled);
        }

        // Merge video segments into a single file
        let video_merged_path = task_temp_dir.join("video_merged.mp4");
        tracing::info!("Merging {} video segments", video_segments.len());
        merge_segments(&video_segments, &video_merged_path).await?;
        cleanup_segments(&video_segments).await;
        let _ = fs::remove_dir(&video_segments_dir).await;

        // Download audio segments if present
        let audio_merged_path = if let Some(audio_rep) = audio_rep {
            tracing::info!("Downloading audio segments ({} total)", audio_rep.segments.len());
            let audio_segments_dir = task_temp_dir.join("audio_segments");
            let audio_segments = segment_downloader
                .download_representation(audio_rep, &audio_segments_dir, progress.clone(), DownloadPhase::DownloadingAudio)
                .await?;

            if self.is_cancelled() {
                cleanup_segments(&audio_segments).await;
                return Err(Error::Cancelled);
            }

            // Merge audio segments
            let audio_path = task_temp_dir.join("audio_merged.mp4");
            tracing::info!("Merging {} audio segments", audio_segments.len());
            merge_segments(&audio_segments, &audio_path).await?;
            cleanup_segments(&audio_segments).await;
            let _ = fs::remove_dir(&audio_segments_dir).await;

            Some(audio_path)
        } else {
            None
        };

        if self.is_cancelled() {
            return Err(Error::Cancelled);
        }

        // Decrypt if DRM protected
        let (video_path, audio_path) = if !decryption_keys.is_empty() {
            {
                let mut p = progress.write().await;
                p.set_phase(DownloadPhase::Decrypting);
            }

            // Create decrypter with keys
            let content_keys: Vec<ContentKey> = decryption_keys
                .iter()
                .map(|(kid, key)| ContentKey {
                    kid: kid.clone(),
                    key: key.clone(),
                    key_type: KeyType::Content,
                })
                .collect();

            let mut decrypter = Decrypter::new(content_keys);
            if let Some(ref mp4decrypt_path) = self.mp4decrypt_path {
                decrypter.set_mp4decrypt_path(mp4decrypt_path.clone());
            }

            // Decrypt video
            tracing::info!("Decrypting video...");
            let decrypted_video = decrypter.decrypt(&video_merged_path).await?;
            let _ = fs::remove_file(&video_merged_path).await;

            // Decrypt audio if present
            let decrypted_audio = if let Some(ref audio_path) = audio_merged_path {
                tracing::info!("Decrypting audio...");
                let decrypted = decrypter.decrypt(audio_path).await?;
                let _ = fs::remove_file(audio_path).await;
                Some(decrypted)
            } else {
                None
            };

            (decrypted_video, decrypted_audio)
        } else {
            (video_merged_path, audio_merged_path)
        };

        if self.is_cancelled() {
            return Err(Error::Cancelled);
        }

        // Download additional audio versions
        let mut additional_audio_tracks: Vec<DownloadedAudioTrack> = Vec::new();
        if !task.additional_audio_versions.is_empty() {
            tracing::info!("Downloading {} additional audio tracks", task.additional_audio_versions.len());
            for (i, version) in task.additional_audio_versions.iter().enumerate() {
                if self.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                tracing::info!("Downloading additional audio {}/{}: {}", i + 1, task.additional_audio_versions.len(), version.audio_locale);
                match self.download_additional_audio(version, auth_token, &task_temp_dir, progress.clone()).await {
                    Ok(track) => additional_audio_tracks.push(track),
                    Err(e) => {
                        tracing::warn!("Failed to download additional audio {}: {}", version.audio_locale, e);
                    }
                }
            }
        }

        // Download subtitles
        let subtitle_paths = if !task.subtitles.is_empty() {
            {
                let mut p = progress.write().await;
                p.set_phase(DownloadPhase::DownloadingSubtitles);
            }
            self.download_subtitles(&task.subtitles, &task_temp_dir).await?
        } else {
            Vec::new()
        };

        if self.is_cancelled() {
            return Err(Error::Cancelled);
        }

        // Mux into final container with subtitles
        {
            let mut p = progress.write().await;
            p.set_phase(DownloadPhase::Muxing);
        }

        let filename = self.generate_filename(&task.episode, video_height);
        let output_path = self.config.output_dir.join(&filename);
        fs::create_dir_all(&self.config.output_dir).await?;

        tracing::info!("Muxing to: {:?}", output_path);

        let muxer = if let Some(ref path) = self.ffmpeg_path {
            Muxer::with_ffmpeg_path(path.clone())
        } else {
            Muxer::new()
        };

        let has_additional_audio = !additional_audio_tracks.is_empty();

        if subtitle_paths.is_empty() && audio_path.is_none() && !has_additional_audio {
            // Just copy/rename the file
            if fs::rename(&video_path, &output_path).await.is_err() {
                fs::copy(&video_path, &output_path).await?;
                let _ = fs::remove_file(&video_path).await;
            }
        } else if has_additional_audio {
            // Multi-audio muxing: collect primary + additional audio tracks
            let mut all_audio_tracks: Vec<DownloadedAudioTrack> = Vec::new();
            if let Some(ref primary_audio) = audio_path {
                let primary_locale = task.primary_audio_locale.clone()
                    .or_else(|| task.episode.audio_locale.clone())
                    .unwrap_or_else(|| "und".to_string());
                all_audio_tracks.push(DownloadedAudioTrack {
                    path: primary_audio.clone(),
                    locale: primary_locale,
                });
            }
            all_audio_tracks.extend(additional_audio_tracks.iter().cloned());

            muxer.mux_multi_audio(
                &video_path,
                &all_audio_tracks,
                &subtitle_paths,
                &output_path,
            ).await?;

            // Clean up temp files
            let _ = fs::remove_file(&video_path).await;
            if let Some(ref ap) = audio_path {
                let _ = fs::remove_file(ap).await;
            }
            for track in &additional_audio_tracks {
                let _ = fs::remove_file(&track.path).await;
            }
        } else {
            muxer.mux_with_subtitles(
                &video_path,
                audio_path.as_deref(),
                &subtitle_paths,
                &output_path,
            ).await?;

            // Clean up temp files
            let _ = fs::remove_file(&video_path).await;
            if let Some(ref ap) = audio_path {
                let _ = fs::remove_file(ap).await;
            }
        }

        for sp in &subtitle_paths {
            let _ = fs::remove_file(&sp.path).await;
        }

        // Clean up the task-specific temp directory
        let _ = fs::remove_dir_all(&task_temp_dir).await;

        // Get file size
        let metadata = fs::metadata(&output_path).await?;
        let size = metadata.len();

        // Update progress
        {
            let mut p = progress.write().await;
            p.set_completed(output_path.to_string_lossy().to_string());
        }

        let subtitle_locales: Vec<String> = subtitle_paths.iter().map(|s| s.locale.clone()).collect();

        let primary_locale = task.primary_audio_locale.clone()
            .or_else(|| task.episode.audio_locale.clone());
        let mut audio_locales: Vec<String> = Vec::new();
        if let Some(ref loc) = primary_locale {
            audio_locales.push(loc.clone());
        }
        for version in &task.additional_audio_versions {
            if !audio_locales.contains(&version.audio_locale) {
                audio_locales.push(version.audio_locale.clone());
            }
        }

        Ok(DownloadResult {
            path: output_path,
            size,
            filename,
            decryption_keys,
            temp_dir: task_temp_dir,
            width: video_width,
            height: video_height,
            subtitle_locales,
            audio_locale: primary_locale,
            audio_locales,
        })
    }

    async fn fetch_manifest(&self, url: &str, auth_token: &str) -> Result<String> {
        let response = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {}", auth_token))
            .send()
            .await
            .map_err(|e| Error::Download(format!("Failed to fetch manifest: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Download(format!(
                "Manifest request failed: {}",
                response.status()
            )));
        }

        response
            .text()
            .await
            .map_err(|e| Error::Download(format!("Failed to read manifest: {}", e)))
    }

    async fn download_subtitles(&self, tracks: &[SubtitleTrack], temp_dir: &PathBuf) -> Result<Vec<DownloadedSubtitle>> {
        let mut subtitles = Vec::new();
        let mut downloaded_locales = std::collections::HashSet::new();

        tracing::info!("Downloading {} subtitle tracks from API...", tracks.len());

        // First, download all subtitles provided by the API
        let mut has_any_subtitle = false;

        for track in tracks {
            // Skip tracks without URL
            let url = match &track.url {
                Some(u) => u,
                None => {
                    tracing::debug!("Skipping subtitle track with no URL");
                    continue;
                }
            };

            let locale = track.locale.as_deref().unwrap_or("unknown").to_string();
            let format = track.format.as_deref().unwrap_or("ass");
            let filename = format!("sub_{}.{}", locale, format);
            let path = temp_dir.join(&filename);

            tracing::info!("Downloading subtitle: {} ({})", locale, format);

            let response = self
                .http
                .get(url)
                .send()
                .await
                .map_err(|e| Error::Download(format!("Subtitle download failed: {}", e)))?;

            if response.status().is_success() {
                let content = response.bytes().await?;
                fs::write(&path, &content).await?;
                tracing::info!("  Downloaded {} bytes for {}", content.len(), locale);
                subtitles.push(DownloadedSubtitle {
                    path,
                    locale: locale.clone(),
                });
                downloaded_locales.insert(locale.clone());
                has_any_subtitle = true;
            } else {
                tracing::warn!("Failed to download subtitle {}: {}", locale, response.status());
            }
        }

        // Note: Crunchyroll subtitle URLs are signed with AWS CloudFront signatures.
        // Each subtitle has a unique ID and cryptographic signature, so we cannot guess URLs
        // for other locales. The available subtitles depend on your account region/IP.
        // To get more subtitles, use a VPN in a region with more subtitle availability (e.g., Vietnam).
        if has_any_subtitle {
            tracing::info!("Subtitle URLs are signed - cannot fetch additional locales via URL guessing");
            tracing::info!("Available subtitles are determined by your account region/IP");
        }

        tracing::info!("Successfully downloaded {} subtitles total", subtitles.len());
        Ok(subtitles)
    }

    /// Download a single additional audio track from a different version's stream
    async fn download_additional_audio(
        &self,
        version: &AudioVersionInfo,
        auth_token: &str,
        task_temp_dir: &PathBuf,
        progress: SharedProgress,
    ) -> Result<DownloadedAudioTrack> {
        tracing::info!("Fetching manifest for additional audio: {}", version.audio_locale);
        let manifest_content = self.fetch_manifest(&version.stream_url, auth_token).await?;

        let parser = DashParser::new(&version.stream_url);
        let parsed = parser.parse(&manifest_content)?;

        let audio_rep = parsed
            .best_audio()
            .ok_or_else(|| Error::Download(format!("No audio representation found for {}", version.audio_locale)))?;

        // Download audio segments
        let audio_dir_name = format!("audio_segments_{}", version.audio_locale);
        let audio_segments_dir = task_temp_dir.join(&audio_dir_name);
        let segment_config = SegmentDownloaderConfig {
            max_concurrent: self.config.max_concurrent_segments,
            retry_count: 3,
            retry_delay_ms: 1000,
        };
        let segment_downloader = SegmentDownloader::new_with_cancelled(self.http.clone(), segment_config, self.cancelled.clone());

        let audio_segments = segment_downloader
            .download_representation(audio_rep, &audio_segments_dir, progress.clone(), DownloadPhase::DownloadingAudio)
            .await?;

        // Merge audio segments
        let audio_merged_name = format!("audio_merged_{}.mp4", version.audio_locale);
        let audio_path = task_temp_dir.join(&audio_merged_name);
        merge_segments(&audio_segments, &audio_path).await?;
        cleanup_segments(&audio_segments).await;
        let _ = fs::remove_dir(&audio_segments_dir).await;

        // Decrypt if DRM protected
        let final_path = if let Some(ref pssh) = version.drm_pssh {
            // Get decryption keys for this version
            let keys = self.get_decryption_keys(pssh, auth_token, version.content_id.as_deref(), version.video_token.as_deref()).await?;

            if !keys.is_empty() {
                let content_keys: Vec<ContentKey> = keys
                    .iter()
                    .map(|(kid, key)| ContentKey {
                        kid: kid.clone(),
                        key: key.clone(),
                        key_type: KeyType::Content,
                    })
                    .collect();

                let mut decrypter = Decrypter::new(content_keys);
                if let Some(ref mp4decrypt_path) = self.mp4decrypt_path {
                    decrypter.set_mp4decrypt_path(mp4decrypt_path.clone());
                }

                let decrypted = decrypter.decrypt(&audio_path).await?;
                let _ = fs::remove_file(&audio_path).await;
                decrypted
            } else {
                audio_path
            }
        } else {
            audio_path
        };

        Ok(DownloadedAudioTrack {
            path: final_path,
            locale: version.audio_locale.clone(),
        })
    }

    /// Get decryption keys (with database caching)
    async fn get_decryption_keys(
        &self,
        pssh: &str,
        auth_token: &str,
        content_id: Option<&str>,
        video_token: Option<&str>,
    ) -> Result<Vec<(String, String)>> {
        // Check database cache first
        if let Some(ref db) = self.database {
            if let Ok(Some(cached)) = db.get_cached_keys(pssh).await {
                tracing::info!("Found cached keys for additional audio PSSH");
                let _ = db.increment_key_use_count(pssh).await;
                return Ok(cached.keys.iter().map(|k| (k.kid.clone(), k.key.clone())).collect());
            }
        }

        // Fetch from license server
        if let Some(ref cdm) = self.cdm {
            let keys = cdm
                .get_content_keys(pssh, None, &self.http, auth_token, content_id, video_token)
                .await?;

            // Save to database
            if let Some(ref db) = self.database {
                let key_pairs: Vec<KeyPair> = keys.iter().map(|k| KeyPair {
                    kid: k.kid.clone(),
                    key: k.key.clone(),
                }).collect();
                let cached = CachedKey::new(pssh.to_string(), key_pairs)
                    .with_content_id(content_id.unwrap_or_default().to_string());
                let _ = db.save_cached_keys(&cached).await;
            }

            Ok(keys.iter().map(|k| (k.kid.clone(), k.key.clone())).collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// Generate filename with smart deduplication:
    /// - If series == season, only show series
    /// - If season == episode title, only show season (or series if series == season)
    /// - If no episode number, omit it
    /// Format: {Series}.{Season}.E{Num}.{Title}.{height}p.mkv (with smart deduplication)
    fn generate_filename(&self, episode: &Episode, video_height: u32) -> String {
        let series = episode
            .series_title
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());

        let season = episode
            .season_title
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());

        let ep_title = &episode.title;

        // Check if episode has a valid number (not "?" or empty)
        let ep_num = episode.display_number();
        let has_valid_ep_num = !ep_num.is_empty() && ep_num != "?";

        // Build filename parts with smart deduplication
        let mut parts = Vec::new();

        // Always add series
        parts.push(series.clone());

        // Add season only if different from series
        if season != series {
            parts.push(season.clone());
        }

        // Add episode number if valid
        if has_valid_ep_num {
            parts.push(format!("E{}", ep_num));
        }

        // Add episode title only if different from both series and season
        if ep_title != &series && ep_title != &season {
            parts.push(ep_title.clone());
        }

        // Add resolution
        parts.push(format!("{}p", video_height));

        let filename = format!("{}.mkv", parts.join("."));
        sanitize_filename(&filename)
    }
}

/// Sanitize a string for use as a filename
fn sanitize_filename(name: &str) -> String {
    let invalid_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let mut result = name.to_string();

    for c in invalid_chars {
        result = result.replace(c, "_");
    }

    // Trim and limit length
    result = result.trim().to_string();
    if result.len() > 200 {
        result = result[..200].to_string();
    }

    result
}

