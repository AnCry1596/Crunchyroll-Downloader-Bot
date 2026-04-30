use crate::download::dash::Representation;
use crate::download::progress::{DownloadPhase, SharedProgress};
use crate::error::{Error, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;

/// Segment download result
#[derive(Debug)]
pub struct SegmentDownloadResult {
    pub path: PathBuf,
    pub size: u64,
}

/// Configuration for segment downloader
#[derive(Debug, Clone)]
pub struct SegmentDownloaderConfig {
    pub max_concurrent: usize,
    pub retry_count: usize,
    pub retry_delay_ms: u64,
}

impl Default for SegmentDownloaderConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 8,
            retry_count: 3,
            retry_delay_ms: 1000,
        }
    }
}

/// Parallel segment downloader with progress tracking
pub struct SegmentDownloader {
    http: wreq::Client,
    config: SegmentDownloaderConfig,
    cancelled: Arc<AtomicBool>,
}

impl SegmentDownloader {
    pub fn new(http: wreq::Client, config: SegmentDownloaderConfig) -> Self {
        Self {
            http,
            config,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn new_with_cancelled(http: wreq::Client, config: SegmentDownloaderConfig, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            http,
            config,
            cancelled,
        }
    }

    /// Cancel ongoing downloads
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Reset cancellation flag
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Download all segments for a representation
    pub async fn download_representation(
        &self,
        rep: &Representation,
        output_dir: &PathBuf,
        progress: SharedProgress,
        phase: DownloadPhase,
    ) -> Result<Vec<SegmentDownloadResult>> {
        self.reset();

        // Create output directory
        fs::create_dir_all(output_dir).await?;

        // Update progress
        {
            let mut p = progress.write().await;
            p.set_phase(phase);
            p.total_segments = rep.segments.len() + if rep.initialization.is_some() { 1 } else { 0 };
            p.completed_segments = 0;
        }

        let mut results = Vec::new();
        let completed_count = Arc::new(AtomicU64::new(0));
        let downloaded_bytes = Arc::new(AtomicU64::new(0));

        // Download initialization segment first if present
        if let Some(ref init_url) = rep.initialization {
            let init_path = output_dir.join("init.mp4");
            let result = self
                .download_segment_with_retry(init_url, &init_path, None)
                .await?;

            downloaded_bytes.fetch_add(result.size, Ordering::SeqCst);
            completed_count.fetch_add(1, Ordering::SeqCst);

            // Update progress
            {
                let mut p = progress.write().await;
                p.completed_segments = completed_count.load(Ordering::SeqCst) as usize;
                p.downloaded_bytes = downloaded_bytes.load(Ordering::SeqCst);
            }

            results.push(result);

            if self.is_cancelled() {
                return Err(Error::Cancelled);
            }
        }

        // Download segments in parallel using semaphore for concurrency control
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_concurrent));
        let mut handles = Vec::new();

        for (idx, segment) in rep.segments.iter().enumerate() {
            if self.is_cancelled() {
                break;
            }

            let sem = semaphore.clone();
            let http = self.http.clone();
            let config = self.config.clone();
            let segment = segment.clone();
            let output_path = output_dir.join(format!("seg_{:05}.m4s", idx));
            let completed = completed_count.clone();
            let bytes = downloaded_bytes.clone();
            let progress = progress.clone();
            let cancelled = self.cancelled.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.map_err(|_| Error::Cancelled)?;

                if cancelled.load(Ordering::SeqCst) {
                    return Err(Error::Cancelled);
                }

                let result = download_segment_internal(
                    &http,
                    &segment.url,
                    &output_path,
                    segment.byte_range,
                    config.retry_count,
                    config.retry_delay_ms,
                    Some(&cancelled),
                )
                .await?;

                // Update counters
                bytes.fetch_add(result.size, Ordering::SeqCst);
                let count = completed.fetch_add(1, Ordering::SeqCst) + 1;

                // Update progress
                {
                    let mut p = progress.write().await;
                    p.completed_segments = count as usize;
                    p.downloaded_bytes = bytes.load(Ordering::SeqCst);
                    p.update_speed();
                }

                Ok::<SegmentDownloadResult, Error>(result)
            });

            handles.push(handle);
        }

        // Wait for all downloads to complete
        for handle in handles {
            match handle.await {
                Ok(Ok(result)) => results.push(result),
                Ok(Err(e)) => {
                    if !self.is_cancelled() {
                        return Err(e);
                    }
                }
                Err(e) => {
                    if !self.is_cancelled() {
                        return Err(Error::Download(format!("Task panicked: {}", e)));
                    }
                }
            }
        }

        if self.is_cancelled() {
            return Err(Error::Cancelled);
        }

        // Sort results by filename to ensure correct order
        results.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(results)
    }

    /// Download a single segment with retry logic
    async fn download_segment_with_retry(
        &self,
        url: &str,
        path: &PathBuf,
        byte_range: Option<(u64, u64)>,
    ) -> Result<SegmentDownloadResult> {
        download_segment_internal(
            &self.http,
            url,
            path,
            byte_range,
            self.config.retry_count,
            self.config.retry_delay_ms,
            Some(&self.cancelled),
        )
        .await
    }
}

/// Internal segment download function with retry
async fn download_segment_internal(
    http: &wreq::Client,
    url: &str,
    path: &PathBuf,
    byte_range: Option<(u64, u64)>,
    retry_count: usize,
    retry_delay_ms: u64,
    cancelled: Option<&Arc<AtomicBool>>,
) -> Result<SegmentDownloadResult> {
    let mut last_error = None;

    for attempt in 0..=retry_count {
        if let Some(c) = cancelled {
            if c.load(Ordering::SeqCst) {
                return Err(Error::Cancelled);
            }
        }

        if attempt > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms)).await;
            tracing::debug!("Retry {} for segment: {}", attempt, url);
        }

        match download_segment_once(http, url, path, byte_range).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                tracing::warn!("Download attempt {} failed for {}: {}", attempt + 1, url, e);
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| Error::Download("Unknown download error".to_string())))
}

/// Download a segment once (no retry)
async fn download_segment_once(
    http: &wreq::Client,
    url: &str,
    path: &PathBuf,
    byte_range: Option<(u64, u64)>,
) -> Result<SegmentDownloadResult> {
    let mut request = http.get(url);

    // Add byte range header if specified
    if let Some((start, end)) = byte_range {
        request = request.header("Range", format!("bytes={}-{}", start, end));
    }

    let response = request
        .send()
        .await
        .map_err(|e| Error::Download(format!("HTTP request failed: {}", e)))?;

    if !response.status().is_success() && response.status().as_u16() != 206 {
        return Err(Error::Download(format!(
            "HTTP error: {} for {}",
            response.status(),
            url
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| Error::Download(format!("Failed to read response: {}", e)))?;

    let size = bytes.len() as u64;

    // Write to file
    let mut file = File::create(path)
        .await
        .map_err(|e| Error::Download(format!("Failed to create file: {}", e)))?;

    file.write_all(&bytes)
        .await
        .map_err(|e| Error::Download(format!("Failed to write file: {}", e)))?;

    file.flush().await?;

    Ok(SegmentDownloadResult {
        path: path.clone(),
        size,
    })
}

/// Merge downloaded segments into a single file
pub async fn merge_segments(
    segments: &[SegmentDownloadResult],
    output_path: &PathBuf,
) -> Result<u64> {
    let mut output = File::create(output_path)
        .await
        .map_err(|e| Error::Download(format!("Failed to create output file: {}", e)))?;

    let mut total_size = 0u64;

    for segment in segments {
        let data = fs::read(&segment.path)
            .await
            .map_err(|e| Error::Download(format!("Failed to read segment: {}", e)))?;

        total_size += data.len() as u64;

        output
            .write_all(&data)
            .await
            .map_err(|e| Error::Download(format!("Failed to write to output: {}", e)))?;
    }

    output.flush().await?;

    Ok(total_size)
}

/// Clean up segment files
pub async fn cleanup_segments(segments: &[SegmentDownloadResult]) {
    for segment in segments {
        let _ = fs::remove_file(&segment.path).await;
    }
}
