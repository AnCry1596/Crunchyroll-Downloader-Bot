use crate::utils::{format_size, format_eta};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Shared progress tracker
pub type SharedProgress = Arc<RwLock<DownloadProgress>>;

/// Create a new shared progress tracker
pub fn new_progress() -> SharedProgress {
    Arc::new(RwLock::new(DownloadProgress::default()))
}

/// Download progress information
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub state: DownloadState,
    pub phase: DownloadPhase,
    pub total_segments: usize,
    pub completed_segments: usize,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub current_speed: f64, // bytes per second
    pub error: Option<String>,
    pub output_file: Option<String>,
    /// Estimated total file size based on bandwidth
    pub estimated_file_size: u64,
    /// Upload progress tracking
    pub upload_bytes: u64,
    pub upload_total_bytes: u64,
    pub upload_speed: f64,
    /// Start time for speed calculation
    start_time: Option<Instant>,
    /// Last update time for speed calculation
    last_update_time: Option<Instant>,
    /// Bytes downloaded at last update
    last_bytes: u64,
    /// Episode title for display
    pub episode_title: Option<String>,
    /// Series title for display
    pub series_title: Option<String>,
    /// Season title for display
    pub season_title: Option<String>,
    /// Episode number for display
    pub episode_number: Option<String>,
}

impl Default for DownloadProgress {
    fn default() -> Self {
        Self {
            state: DownloadState::default(),
            phase: DownloadPhase::default(),
            total_segments: 0,
            completed_segments: 0,
            total_bytes: 0,
            downloaded_bytes: 0,
            current_speed: 0.0,
            error: None,
            output_file: None,
            estimated_file_size: 0,
            upload_bytes: 0,
            upload_total_bytes: 0,
            upload_speed: 0.0,
            start_time: None,
            last_update_time: None,
            last_bytes: 0,
            episode_title: None,
            series_title: None,
            season_title: None,
            episode_number: None,
        }
    }
}

impl DownloadProgress {
    /// Get progress percentage (0-100)
    pub fn percentage(&self) -> f32 {
        if self.total_segments == 0 {
            return 0.0;
        }
        (self.completed_segments as f32 / self.total_segments as f32) * 100.0
    }

    /// Start timing for speed calculation
    pub fn start_timing(&mut self) {
        let now = Instant::now();
        self.start_time = Some(now);
        self.last_update_time = Some(now);
        self.last_bytes = 0;
    }

    /// Update speed calculation based on bytes downloaded
    pub fn update_speed(&mut self) {
        let now = Instant::now();
        if let Some(last_time) = self.last_update_time {
            let elapsed = now.duration_since(last_time).as_secs_f64();
            if elapsed >= 0.5 {
                // Update speed every 0.5 seconds minimum
                let bytes_diff = self.downloaded_bytes.saturating_sub(self.last_bytes);
                self.current_speed = bytes_diff as f64 / elapsed;
                self.last_update_time = Some(now);
                self.last_bytes = self.downloaded_bytes;
            }
        }
    }

    /// Update upload speed calculation
    pub fn update_upload_speed(&mut self, bytes_uploaded: u64) {
        let now = Instant::now();
        if let Some(last_time) = self.last_update_time {
            let elapsed = now.duration_since(last_time).as_secs_f64();
            if elapsed >= 0.5 {
                let bytes_diff = bytes_uploaded.saturating_sub(self.upload_bytes);
                self.upload_speed = bytes_diff as f64 / elapsed;
                self.last_update_time = Some(now);
            }
        } else {
            self.last_update_time = Some(now);
        }
        self.upload_bytes = bytes_uploaded;
    }

    /// Build the episode header for progress messages
    /// Shows: Series | Season | Episode Title
    fn format_header(&self) -> String {
        let mut lines = Vec::new();

        // Line 1: Series title
        if let Some(ref series) = self.series_title {
            lines.push(format!("📺 {}", series));
        }

        // Line 2: Season title
        if let Some(ref season) = self.season_title {
            lines.push(format!("📁 {}", season));
        }

        // Line 3: Episode number + title
        // Only show episode number if it's a valid number (not "?" or empty)
        let has_valid_ep_num = self.episode_number.as_ref()
            .map(|n| !n.is_empty() && n != "?")
            .unwrap_or(false);

        let ep_line = match (&self.episode_number, &self.episode_title, has_valid_ep_num) {
            (Some(ep_num), Some(ep_title), true) => {
                format!("🎬 E{} - {}", ep_num, ep_title)
            }
            (_, Some(ep_title), _) => {
                format!("🎬 {}", ep_title)
            }
            (Some(ep_num), None, true) => {
                format!("🎬 E{}", ep_num)
            }
            _ => String::new(),
        };

        if !ep_line.is_empty() {
            lines.push(ep_line);
        }

        if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n\n", lines.join("\n"))
        }
    }

    /// Build a progress bar string
    fn progress_bar(&self, width: usize) -> String {
        let pct = self.percentage() / 100.0;
        let filled = (pct * width as f32) as usize;
        let empty = width.saturating_sub(filled);
        format!("[{}{}]", "▓".repeat(filled), "░".repeat(empty))
    }

    /// Get formatted progress string
    pub fn format_progress(&self) -> String {
        let header = self.format_header();

        match &self.phase {
            DownloadPhase::Idle => format!("{}⏳ Đang chờ...", header),
            DownloadPhase::FetchingManifest => format!("{}📋 Đang lấy manifest...", header),
            DownloadPhase::FetchingKeys => format!("{}🔑 Đang lấy khóa giải mã...", header),
            DownloadPhase::DownloadingVideo => {
                let mut status = format!(
                    "{}🎬 Đang tải Video\n{} {:.1}%\n📊 {}/{} segments",
                    header,
                    self.progress_bar(20),
                    self.percentage(),
                    self.completed_segments,
                    self.total_segments,
                );
                if self.current_speed > 0.0 {
                    status.push_str(&format!("\n⚡ Tốc độ: {}", self.speed_string()));
                }
                if self.estimated_file_size > 0 {
                    status.push_str(&format!("\n📦 Ước tính: ~{}", format_size(self.estimated_file_size)));
                }
                if self.downloaded_bytes > 0 {
                    status.push_str(&format!("\n⬇️ Đã tải: {}", format_size(self.downloaded_bytes)));
                }
                status
            }
            DownloadPhase::DownloadingAudio => {
                let mut status = format!(
                    "{}🔊 Đang tải Audio\n{} {:.1}%\n📊 {}/{} segments",
                    header,
                    self.progress_bar(20),
                    self.percentage(),
                    self.completed_segments,
                    self.total_segments,
                );
                if self.current_speed > 0.0 {
                    status.push_str(&format!("\n⚡ Tốc độ: {}", self.speed_string()));
                }
                if self.downloaded_bytes > 0 {
                    status.push_str(&format!("\n⬇️ Đã tải: {}", format_size(self.downloaded_bytes)));
                }
                status
            }
            DownloadPhase::DownloadingSubtitles => format!("{}📝 Đang tải phụ đề...", header),
            DownloadPhase::Decrypting => format!("{}🔓 Đang giải mã nội dung...", header),
            DownloadPhase::Muxing => format!("{}🎞️ Đang ghép video, audio & phụ đề...", header),
            DownloadPhase::Uploading { progress } => {
                let mut status = format!(
                    "{}📤 Đang tải lên Telegram\n{} {:.1}%",
                    header,
                    self.progress_bar(20),
                    progress
                );
                if self.upload_speed > 0.0 {
                    status.push_str(&format!("\n⚡ Tốc độ: {}", self.upload_speed_string()));
                }
                if self.upload_total_bytes > 0 {
                    status.push_str(&format!(
                        "\n📊 Tiến độ: {} / {}",
                        format_size(self.upload_bytes),
                        format_size(self.upload_total_bytes)
                    ));
                }
                if self.upload_speed > 0.0 && self.upload_total_bytes > 0 && self.upload_bytes < self.upload_total_bytes {
                    let remaining = self.upload_total_bytes.saturating_sub(self.upload_bytes);
                    let eta_secs = remaining as f64 / self.upload_speed;
                    status.push_str(&format!("\n⏳ ETA: {}", format_eta(eta_secs)));
                }
                status
            }
            DownloadPhase::Completed => format!("{}✅ Tải xuống hoàn tất!", header),
            DownloadPhase::Failed => {
                format!("{}❌ Thất bại: {}", header, self.error.as_deref().unwrap_or("Lỗi không xác định"))
            }
        }
    }

    /// Get speed string
    pub fn speed_string(&self) -> String {
        Self::format_speed(self.current_speed)
    }

    /// Get upload speed string
    pub fn upload_speed_string(&self) -> String {
        Self::format_speed(self.upload_speed)
    }

    /// Format speed for display
    fn format_speed(speed: f64) -> String {
        if speed <= 0.0 {
            return "-- MB/s".to_string();
        }

        if speed >= 1_000_000.0 {
            format!("{:.1} MB/s", speed / 1_000_000.0)
        } else if speed >= 1_000.0 {
            format!("{:.1} KB/s", speed / 1_000.0)
        } else {
            format!("{:.0} B/s", speed)
        }
    }

    /// Update state
    pub fn set_state(&mut self, state: DownloadState) {
        self.state = state;
    }

    /// Update phase
    pub fn set_phase(&mut self, phase: DownloadPhase) {
        self.phase = phase;
    }

    /// Set error and mark as failed
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.state = DownloadState::Failed;
        self.phase = DownloadPhase::Failed;
    }

    /// Mark as completed
    pub fn set_completed(&mut self, output_file: impl Into<String>) {
        self.output_file = Some(output_file.into());
        self.state = DownloadState::Completed;
        self.phase = DownloadPhase::Completed;
    }

    /// Set episode information for progress display
    pub fn set_episode_info(
        &mut self,
        series_title: Option<String>,
        season_title: Option<String>,
        episode_title: Option<String>,
        episode_number: Option<String>,
    ) {
        self.series_title = series_title;
        self.season_title = season_title;
        self.episode_title = episode_title;
        self.episode_number = episode_number;
    }
}

/// Download state
#[derive(Debug, Clone, Default, PartialEq)]
pub enum DownloadState {
    #[default]
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

/// Current download phase
#[derive(Debug, Clone, Default)]
pub enum DownloadPhase {
    #[default]
    Idle,
    FetchingManifest,
    FetchingKeys,
    DownloadingVideo,
    DownloadingAudio,
    DownloadingSubtitles,
    Decrypting,
    Muxing,
    Uploading { progress: f32 },
    Completed,
    Failed,
}

/// Progress callback trait
#[async_trait::async_trait]
pub trait ProgressCallback: Send + Sync {
    async fn on_progress(&self, progress: &DownloadProgress);
    async fn on_state_change(&self, old: DownloadState, new: DownloadState);
    async fn on_completed(&self, output_file: &str);
    async fn on_error(&self, error: &str);
}
