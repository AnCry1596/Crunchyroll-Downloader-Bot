use crate::utils::format::format_eta;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Upload progress tracking for various upload services
#[derive(Clone)]
pub struct UploadProgress {
    pub file_size: u64,
    pub uploaded_bytes: u64,
    pub start_time: Instant,
    pub is_complete: bool,
    pub service_name: String,
    pub upload_speed_mbps: f64,
}

impl UploadProgress {
    pub fn new(file_size: u64, service_name: &str) -> Self {
        Self {
            file_size,
            uploaded_bytes: 0,
            start_time: Instant::now(),
            is_complete: false,
            service_name: service_name.to_string(),
            upload_speed_mbps: 0.0,
        }
    }

    /// Get elapsed time in seconds
    pub fn elapsed_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Update progress with bytes uploaded
    pub fn update(&mut self, uploaded_bytes: u64) {
        self.uploaded_bytes = uploaded_bytes;
        let elapsed = self.elapsed_secs();
        if elapsed > 0.0 {
            self.upload_speed_mbps = (uploaded_bytes as f64 / 1_048_576.0) / elapsed * 8.0;
        }
    }

    /// Mark as complete and calculate final speed
    pub fn complete(&mut self, final_speed_mbps: f64) {
        self.is_complete = true;
        self.uploaded_bytes = self.file_size;
        self.upload_speed_mbps = final_speed_mbps;
    }

    /// Format progress message for display
    pub fn format_message(&self) -> String {
        let elapsed = self.elapsed_secs();
        let size_mb = self.file_size as f64 / 1_048_576.0;
        let uploaded_mb = self.uploaded_bytes as f64 / 1_048_576.0;

        // Calculate speed only if we have actual upload progress data
        let speed_display = if self.upload_speed_mbps > 0.0 {
            let mb_per_sec = self.upload_speed_mbps / 8.0;
            format!("{:.1} MB/s ({:.0} Mbps)", mb_per_sec, self.upload_speed_mbps)
        } else if self.uploaded_bytes > 0 && elapsed > 0.5 {
            // Calculate actual speed from uploaded bytes
            let actual_speed_mbs = uploaded_mb / elapsed;
            let actual_speed_mbps = actual_speed_mbs * 8.0;
            format!("~{:.1} MB/s (~{:.0} Mbps)", actual_speed_mbs, actual_speed_mbps)
        } else {
            // No progress data available - don't estimate
            "⏳ Đang tính...".to_string()
        };

        let progress_pct = if self.file_size > 0 {
            (self.uploaded_bytes as f64 / self.file_size as f64) * 100.0
        } else {
            0.0
        };

        if self.uploaded_bytes > 0 && self.uploaded_bytes < self.file_size {
            let remaining = self.file_size.saturating_sub(self.uploaded_bytes);
            let speed_bytes_per_sec = self.upload_speed_mbps / 8.0 * 1_048_576.0;
            let eta_display = if speed_bytes_per_sec > 0.0 {
                format_eta(remaining as f64 / speed_bytes_per_sec)
            } else {
                "⏳ Đang tính...".to_string()
            };

            format!(
                "📤 Đang tải lên {}...\n\n\
                📦 Kích thước: {:.2} MB\n\
                📊 Tiến độ: {:.2} / {:.2} MB ({:.1}%)\n\
                ⚡ Tốc độ: {}\n\
                ⏳ ETA: {}\n\
                ⏱ Thời gian: {:.0}s\n\n\
                ⚠️ Không thể huỷ tải lên",
                self.service_name,
                size_mb,
                uploaded_mb,
                size_mb,
                progress_pct,
                speed_display,
                eta_display,
                elapsed
            )
        } else {
            format!(
                "📤 Đang tải lên {}...\n\n\
                📦 Kích thước: {:.2} MB\n\
                ⚡ Tốc độ: {}\n\
                ⏱ Thời gian: {:.0}s\n\n\
                ⚠️ Không thể huỷ tải lên",
                self.service_name,
                size_mb,
                speed_display,
                elapsed
            )
        }
    }
}

pub type SharedUploadProgress = Arc<RwLock<UploadProgress>>;

pub fn new_upload_progress(file_size: u64, service_name: &str) -> SharedUploadProgress {
    Arc::new(RwLock::new(UploadProgress::new(file_size, service_name)))
}