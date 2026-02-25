use crate::download::manager::{DownloadedAudioTrack, DownloadedSubtitle};
use crate::drm::widevine::{ContentKey, KeyType};
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

/// Decrypter for DRM-protected content
pub struct Decrypter {
    keys: Vec<ContentKey>,
    mp4decrypt_path: Option<PathBuf>,
}

impl Decrypter {
    pub fn new(keys: Vec<ContentKey>) -> Self {
        Self { keys, mp4decrypt_path: None }
    }

    /// Create decrypter with custom mp4decrypt path
    pub fn with_mp4decrypt_path(keys: Vec<ContentKey>, mp4decrypt_path: PathBuf) -> Self {
        Self { keys, mp4decrypt_path: Some(mp4decrypt_path) }
    }

    /// Set mp4decrypt path
    pub fn set_mp4decrypt_path(&mut self, path: PathBuf) {
        self.mp4decrypt_path = Some(path);
    }

    /// Get content keys only
    pub fn content_keys(&self) -> Vec<&ContentKey> {
        self.keys
            .iter()
            .filter(|k| k.key_type == KeyType::Content)
            .collect()
    }

    /// Get content keys as (kid, key) hex string pairs
    pub fn content_keys_hex(&self) -> Vec<(String, String)> {
        self.content_keys()
            .iter()
            .map(|k| (hex::encode(&k.kid), hex::encode(&k.key)))
            .collect()
    }

    /// Decrypt a file using mp4decrypt (Bento4)
    pub async fn decrypt_with_mp4decrypt(&self, input: &Path, output: &Path) -> Result<()> {
        let content_keys = self.content_keys();
        if content_keys.is_empty() {
            return Err(Error::NoContentKeys);
        }

        let num_keys = content_keys.len();

        // Determine mp4decrypt executable path
        let mp4decrypt_cmd = if let Some(ref path) = self.mp4decrypt_path {
            if path.exists() {
                path.to_string_lossy().to_string()
            } else {
                // Fall back to system PATH
                "mp4decrypt".to_string()
            }
        } else {
            "mp4decrypt".to_string()
        };

        // Build command arguments
        let mut args: Vec<String> = Vec::new();

        // Add key arguments
        for key in content_keys {
            args.push("--key".to_string());
            args.push(key.to_mp4decrypt_arg());
        }

        // Add input and output files
        args.push(input.to_string_lossy().to_string());
        args.push(output.to_string_lossy().to_string());

        tracing::info!("Running mp4decrypt ({}) with {} keys", mp4decrypt_cmd, num_keys);

        let output_result = Command::new(&mp4decrypt_cmd)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::Mp4DecryptNotFound
                } else {
                    Error::ExternalTool {
                        tool: "mp4decrypt".to_string(),
                        message: format!("Failed to execute: {}", e),
                    }
                }
            })?;

        if !output_result.status.success() {
            let stderr = String::from_utf8_lossy(&output_result.stderr);
            tracing::error!("mp4decrypt stderr: {}", stderr);
            let exit_code = output_result.status.code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(Error::Decryption(format!(
                "mp4decrypt thoát với mã lỗi {}: {}",
                exit_code,
                stderr.lines().last().unwrap_or("Không rõ lỗi")
            )));
        }

        tracing::debug!("Decryption completed: {:?}", output);
        Ok(())
    }

    /// Decrypt a file using shaka-packager (alternative)
    pub async fn decrypt_with_shaka(&self, input: &Path, output: &Path) -> Result<()> {
        let content_keys = self.content_keys();
        if content_keys.is_empty() {
            return Err(Error::NoContentKeys);
        }

        // Take the first content key
        let key = content_keys.first().unwrap();

        let input_spec = format!(
            "input={},stream=video,output={}",
            input.display(),
            output.display()
        );

        let keys_spec = format!("key_id={}:key={}", key.kid, key.key);

        let status = Command::new("packager")
            .args(&[
                &input_spec,
                "--enable_raw_key_decryption",
                &format!("--keys={}", keys_spec),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| Error::ExternalTool {
                tool: "shaka-packager".to_string(),
                message: format!("Failed to execute: {}", e),
            })?;

        if !status.success() {
            let exit_code = status.code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(Error::Decryption(format!(
                "shaka-packager thoát với mã lỗi {}",
                exit_code
            )));
        }

        Ok(())
    }

    /// Decrypt a file, trying mp4decrypt first, then shaka-packager
    pub async fn decrypt(&self, input: &Path) -> Result<PathBuf> {
        let output = input.with_extension("decrypted.mp4");

        // Try mp4decrypt first
        match self.decrypt_with_mp4decrypt(input, &output).await {
            Ok(()) => return Ok(output),
            Err(Error::Mp4DecryptNotFound) => {
                tracing::warn!("mp4decrypt not found, trying shaka-packager");
            }
            Err(e) => return Err(e),
        }

        // Fallback to shaka-packager
        self.decrypt_with_shaka(input, &output).await?;
        Ok(output)
    }

}

/// Mux video, audio, and subtitles using FFmpeg
pub struct Muxer {
    ffmpeg_path: Option<PathBuf>,
}

impl Default for Muxer {
    fn default() -> Self {
        Self::new()
    }
}

impl Muxer {
    pub fn new() -> Self {
        Self { ffmpeg_path: None }
    }

    /// Create muxer with custom ffmpeg path
    pub fn with_ffmpeg_path(ffmpeg_path: PathBuf) -> Self {
        Self { ffmpeg_path: Some(ffmpeg_path) }
    }

    /// Get ffmpeg command
    fn ffmpeg_cmd(&self) -> String {
        if let Some(ref path) = self.ffmpeg_path {
            if path.exists() {
                return path.to_string_lossy().to_string();
            }
        }
        "ffmpeg".to_string()
    }

    /// Mux video and audio into a single container
    pub async fn mux_video_audio(
        &self,
        video: &Path,
        audio: Option<&Path>,
        output: &Path,
    ) -> Result<()> {
        let ffmpeg = self.ffmpeg_cmd();

        let mut args = vec![
            "-y".to_string(),
            "-i".to_string(),
            video.to_string_lossy().to_string(),
        ];

        if let Some(audio_path) = audio {
            args.push("-i".to_string());
            args.push(audio_path.to_string_lossy().to_string());
        }

        args.extend([
            "-c".to_string(),
            "copy".to_string(),
            output.to_string_lossy().to_string(),
        ]);

        tracing::info!("Running ffmpeg ({})", ffmpeg);

        let output_result = Command::new(&ffmpeg)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::FfmpegNotFound
                } else {
                    Error::Muxing(format!("Failed to execute ffmpeg: {}", e))
                }
            })?;

        if !output_result.status.success() {
            let stderr = String::from_utf8_lossy(&output_result.stderr);
            tracing::error!("ffmpeg stderr: {}", stderr);
            let exit_code = output_result.status.code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(Error::Muxing(format!(
                "FFmpeg thoát với mã lỗi {} - {}",
                exit_code,
                stderr.lines().last().unwrap_or("Không rõ lỗi")
            )));
        }

        Ok(())
    }

    /// Mux video, audio, and subtitles into MKV container
    pub async fn mux_with_subtitles(
        &self,
        video: &Path,
        audio: Option<&Path>,
        subtitles: &[DownloadedSubtitle],
        output: &Path,
    ) -> Result<()> {
        let ffmpeg = self.ffmpeg_cmd();

        let mut args = vec![
            "-y".to_string(),
            "-i".to_string(),
            video.to_string_lossy().to_string(),
        ];

        // Add audio input if present
        if let Some(audio_path) = audio {
            args.push("-i".to_string());
            args.push(audio_path.to_string_lossy().to_string());
        }

        // Add subtitle inputs
        for sub in subtitles {
            args.push("-i".to_string());
            args.push(sub.path.to_string_lossy().to_string());
        }

        // Map all streams
        let input_count = 1 + audio.is_some() as usize + subtitles.len();
        for i in 0..input_count {
            args.push("-map".to_string());
            args.push(i.to_string());
        }

        // Copy codecs, keep subtitles in original format (ASS)
        args.extend([
            "-c:v".to_string(),
            "copy".to_string(),
            "-c:a".to_string(),
            "copy".to_string(),
            "-c:s".to_string(),
            "copy".to_string(), // Keep subtitles in original format (ASS/SSA)
        ]);

        // Set subtitle language metadata
        for (i, sub) in subtitles.iter().enumerate() {
            // Set language metadata for each subtitle track
            args.push(format!("-metadata:s:s:{}", i));
            args.push(format!("language={}", sub.locale));
            args.push(format!("-metadata:s:s:{}", i));
            args.push(format!("title={}", Self::locale_to_name(&sub.locale)));
        }

        args.push(output.to_string_lossy().to_string());

        tracing::info!("Running ffmpeg ({}) for muxing with subtitles", ffmpeg);

        let output_result = Command::new(&ffmpeg)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::FfmpegNotFound
                } else {
                    Error::Muxing(format!("Failed to execute ffmpeg: {}", e))
                }
            })?;

        if !output_result.status.success() {
            let stderr = String::from_utf8_lossy(&output_result.stderr);
            tracing::error!("ffmpeg stderr: {}", stderr);
            let exit_code = output_result.status.code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(Error::Muxing(format!(
                "FFmpeg thoát với mã lỗi {} - {}",
                exit_code,
                stderr.lines().last().unwrap_or("Không rõ lỗi")
            )));
        }

        Ok(())
    }

    /// Mux video + multiple audio tracks + subtitles into MKV container
    pub async fn mux_multi_audio(
        &self,
        video: &Path,
        audio_tracks: &[DownloadedAudioTrack],
        subtitles: &[DownloadedSubtitle],
        output: &Path,
    ) -> Result<()> {
        let ffmpeg = self.ffmpeg_cmd();

        let mut args = vec![
            "-y".to_string(),
            "-i".to_string(),
            video.to_string_lossy().to_string(),
        ];

        // Add audio inputs
        for track in audio_tracks {
            args.push("-i".to_string());
            args.push(track.path.to_string_lossy().to_string());
        }

        // Add subtitle inputs
        for sub in subtitles {
            args.push("-i".to_string());
            args.push(sub.path.to_string_lossy().to_string());
        }

        // Map all streams
        let input_count = 1 + audio_tracks.len() + subtitles.len();
        for i in 0..input_count {
            args.push("-map".to_string());
            args.push(i.to_string());
        }

        // Copy codecs
        args.extend([
            "-c:v".to_string(), "copy".to_string(),
            "-c:a".to_string(), "copy".to_string(),
            "-c:s".to_string(), "copy".to_string(),
        ]);

        // Audio metadata
        for (i, track) in audio_tracks.iter().enumerate() {
            let iso_lang = Self::locale_to_iso639(&track.locale);
            let display = Self::locale_to_name(&track.locale);
            args.push(format!("-metadata:s:a:{}", i));
            args.push(format!("language={}", iso_lang));
            args.push(format!("-metadata:s:a:{}", i));
            args.push(format!("title={}", display));
        }

        // Mark first audio as default
        if !audio_tracks.is_empty() {
            args.push("-disposition:a:0".to_string());
            args.push("default".to_string());
        }

        // Subtitle metadata
        for (i, sub) in subtitles.iter().enumerate() {
            args.push(format!("-metadata:s:s:{}", i));
            args.push(format!("language={}", Self::locale_to_iso639(&sub.locale)));
            args.push(format!("-metadata:s:s:{}", i));
            args.push(format!("title={}", Self::locale_to_name(&sub.locale)));
        }

        args.push(output.to_string_lossy().to_string());

        tracing::info!("Running ffmpeg ({}) for multi-audio muxing ({} audio tracks, {} subtitles)",
            ffmpeg, audio_tracks.len(), subtitles.len());

        let output_result = Command::new(&ffmpeg)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::FfmpegNotFound
                } else {
                    Error::Muxing(format!("Failed to execute ffmpeg: {}", e))
                }
            })?;

        if !output_result.status.success() {
            let stderr = String::from_utf8_lossy(&output_result.stderr);
            tracing::error!("ffmpeg stderr: {}", stderr);
            let exit_code = output_result.status.code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(Error::Muxing(format!(
                "FFmpeg thoát với mã lỗi {} - {}",
                exit_code,
                stderr.lines().last().unwrap_or("Không rõ lỗi")
            )));
        }

        Ok(())
    }

    /// Convert Crunchyroll locale to ISO 639-2/B code for MKV metadata
    pub fn locale_to_iso639(locale: &str) -> &'static str {
        match locale {
            "ja-JP" => "jpn",
            "en-US" | "en-GB" | "en-IN" => "eng",
            "es-ES" | "es-LA" | "es-419" => "spa",
            "pt-BR" | "pt-PT" => "por",
            "fr-FR" => "fre",
            "de-DE" => "ger",
            "it-IT" => "ita",
            "ru-RU" => "rus",
            "ko-KR" => "kor",
            "zh-CN" => "chi",
            "zh-TW" => "chi",
            "ar-SA" | "ar-ME" => "ara",
            "hi-IN" => "hin",
            "id-ID" => "ind",
            "ms-MY" => "may",
            "th-TH" => "tha",
            "vi-VN" => "vie",
            "pl-PL" => "pol",
            "tr-TR" => "tur",
            "nl-NL" => "dut",
            "sv-SE" => "swe",
            "da-DK" => "dan",
            "fi-FI" => "fin",
            "no-NO" => "nor",
            "cs-CZ" => "cze",
            "hu-HU" => "hun",
            "ro-RO" => "rum",
            "el-GR" => "gre",
            "he-IL" => "heb",
            "uk-UA" => "ukr",
            "ta-IN" => "tam",
            "te-IN" => "tel",
            _ => "und",
        }
    }

    /// Convert locale code to human-readable language name
    pub fn locale_to_name(locale: &str) -> String {
        match locale {
            "en-US" => "English (US)".to_string(),
            "en-GB" => "English (UK)".to_string(),
            "es-ES" => "Spanish (Spain)".to_string(),
            "es-LA" | "es-419" => "Spanish (Latin America)".to_string(),
            "pt-BR" => "Portuguese (Brazil)".to_string(),
            "pt-PT" => "Portuguese (Portugal)".to_string(),
            "fr-FR" => "French".to_string(),
            "de-DE" => "German".to_string(),
            "it-IT" => "Italian".to_string(),
            "ru-RU" => "Russian".to_string(),
            "ja-JP" => "Japanese".to_string(),
            "ko-KR" => "Korean".to_string(),
            "zh-CN" => "Chinese (Simplified)".to_string(),
            "zh-TW" => "Chinese (Traditional)".to_string(),
            "ar-SA" | "ar-ME" => "Arabic".to_string(),
            "hi-IN" => "Hindi".to_string(),
            "id-ID" => "Indonesian".to_string(),
            "ms-MY" => "Malay".to_string(),
            "th-TH" => "Thai".to_string(),
            "vi-VN" => "Vietnamese".to_string(),
            "pl-PL" => "Polish".to_string(),
            "tr-TR" => "Turkish".to_string(),
            "nl-NL" => "Dutch".to_string(),
            "sv-SE" => "Swedish".to_string(),
            "da-DK" => "Danish".to_string(),
            "fi-FI" => "Finnish".to_string(),
            "no-NO" => "Norwegian".to_string(),
            "cs-CZ" => "Czech".to_string(),
            "hu-HU" => "Hungarian".to_string(),
            "ro-RO" => "Romanian".to_string(),
            "el-GR" => "Greek".to_string(),
            "he-IL" => "Hebrew".to_string(),
            "uk-UA" => "Ukrainian".to_string(),
            _ => locale.to_string(), // Return the locale code if unknown
        }
    }
}
