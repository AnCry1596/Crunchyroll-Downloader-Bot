use crate::error::{Error, Result};
use crate::utils::{SharedUploadProgress, bytes_to_mb, upload_speed_mbps};
use futures_util::TryStreamExt;
use serde::Deserialize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use wreq::multipart;

const PIXELDRAIN_UPLOAD_URL: &str = "https://pixeldrain.com/api/file";

#[derive(Debug, Deserialize)]
struct PixeldrainResponse {
    success: bool,
    id: Option<String>,
    message: Option<String>,
}

/// Upload result with speed information
pub struct PixeldrainUploadResult {
    pub download_link: String,
    pub file_id: String,
    pub upload_speed_mbps: f64,
    pub duration_secs: f64,
}

/// Upload a file to Pixeldrain and return the download link, file ID, and upload speed
pub async fn upload_to_pixeldrain(
    file_path: &Path,
    filename: &str,
    api_key: Option<&str>,
    progress: Option<SharedUploadProgress>,
) -> Result<PixeldrainUploadResult> {
    let file = File::open(file_path).await.map_err(|e| {
        Error::Upload(format!("Failed to open file for Pixeldrain upload: {}", e))
    })?;

    let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

    tracing::info!(
        "Uploading {} ({} bytes / {:.2} MB) to Pixeldrain...",
        filename,
        file_size,
        bytes_to_mb(file_size)
    );

    // Build client with timeout (1 hour for large files)
    let client = wreq::Client::builder()
        .timeout(Duration::from_secs(60 * 60)) // 1 hour timeout
        .connect_timeout(Duration::from_secs(30)) // 30 seconds connect timeout
        .build()
        .map_err(|e| Error::Upload(format!("Failed to create HTTP client: {}", e)))?;

    // Start timing the upload
    let start_time = Instant::now();

    // Track uploaded bytes for progress
    let uploaded_bytes = Arc::new(AtomicU64::new(0));
    let uploaded_bytes_clone = uploaded_bytes.clone();
    let progress_clone = progress.clone();

    // Create streaming body with progress tracking
    let stream = ReaderStream::new(file).inspect_ok(move |bytes| {
        let new_total = uploaded_bytes_clone.fetch_add(bytes.len() as u64, Ordering::SeqCst) + bytes.len() as u64;

        // Update progress if available
        if let Some(ref p) = progress_clone {
            // Use try_write to avoid blocking - we don't want to slow down the upload
            if let Ok(mut prog) = p.try_write() {
                prog.update(new_total);
            }
        }
    });

    // Create body from stream with content length hint
    let body = wreq::Body::wrap_stream(stream);

    // Build multipart form with streaming body
    let form = multipart::Form::new()
        .part("file", multipart::Part::stream_with_length(body, file_size)
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| Error::Upload(format!("Failed to set MIME type: {}", e)))?
        );

    let mut request = client.post(PIXELDRAIN_UPLOAD_URL).multipart(form);

    // Add API key authentication if provided
    if let Some(key) = api_key {
        request = request.basic_auth("", Some(key));
    }

    let response = request
        .send()
        .await
        .map_err(|e| Error::Upload(format!("Pixeldrain upload failed: {}", e)))?;

    let duration = start_time.elapsed();
    let duration_secs = duration.as_secs_f64();
    let speed = upload_speed_mbps(file_size, duration_secs);

    // Mark upload as complete with final speed
    if let Some(ref p) = progress {
        let mut prog = p.write().await;
        prog.complete(speed);
    }

    let status = response.status();
    let body = response.text().await.map_err(|e| {
        Error::Upload(format!("Failed to read Pixeldrain response: {}", e))
    })?;

    // Print response for debugging
    tracing::info!("=== Pixeldrain Response ===");
    tracing::info!("Status: {}", status);
    tracing::info!("Body: {}", body);
    tracing::info!("Upload duration: {:.2}s", duration_secs);
    tracing::info!("Upload speed: {:.2} Mbps ({:.2} MB/s)", speed, speed / 8.0);
    tracing::info!("===========================");

    if !status.is_success() {
        return Err(Error::Upload(format!(
            "Pixeldrain upload failed ({}): {}",
            status, body
        )));
    }

    let result: PixeldrainResponse = serde_json::from_str(&body).map_err(|e| {
        Error::Upload(format!("Failed to parse Pixeldrain response: {}", e))
    })?;

    if !result.success {
        return Err(Error::Upload(format!(
            "Pixeldrain upload failed: {}",
            result.message.unwrap_or_else(|| "Unknown error".to_string())
        )));
    }

    let file_id = result.id.ok_or_else(|| {
        Error::Upload("Pixeldrain response missing file ID".to_string())
    })?;

    let download_link = format!("https://pixeldrain.com/u/{}", file_id);
    tracing::info!("Successfully uploaded to Pixeldrain: {}", download_link);

    Ok(PixeldrainUploadResult {
        download_link,
        file_id,
        upload_speed_mbps: speed,
        duration_secs,
    })
}
