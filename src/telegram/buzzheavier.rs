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
use wreq::Body;

// Re-export for backwards compatibility
pub use crate::utils::new_upload_progress;

/// Response from Buzzheavier upload
/// Actual response format: {"code":201,"data":{"id":"xxx","name":"...","isDirectory":false,...}}
#[derive(Debug, Deserialize)]
struct BuzzheavierResponse {
    #[serde(default)]
    code: Option<i32>,
    #[serde(default)]
    data: Option<BuzzheavierData>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BuzzheavierData {
    id: String,
    #[allow(dead_code)]
    #[serde(default)]
    name: Option<String>,
}


/// Upload result with speed information
pub struct UploadResult {
    pub download_link: String,
    pub file_id: String,
    pub upload_speed_mbps: f64,
    pub duration_secs: f64,
}

/// Upload a file to Buzzheavier and return the download link, file ID, and upload speed
///
/// Uses PUT request with Bearer token authentication:
/// `curl -T "sample.mp4" -H "Authorization: Bearer YOUR_ACCOUNT_ID" "https://w.buzzheavier.com/{parentId}/sample.mp4"`
pub async fn upload_to_buzzheavier(
    file_path: &Path,
    filename: &str,
    account_id: &str,
    parent_id: Option<&str>,
    progress: Option<SharedUploadProgress>,
) -> Result<UploadResult> {
    let file = File::open(file_path).await.map_err(|e| {
        Error::Upload(format!("Failed to open file for Buzzheavier upload: {}", e))
    })?;

    let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

    tracing::info!(
        "Uploading {} ({} bytes / {:.2} MB) to Buzzheavier...",
        filename,
        file_size,
        bytes_to_mb(file_size)
    );

    // Build the upload URL
    // If parent_id is provided: https://w.buzzheavier.com/{parentId}/{filename}
    // Otherwise: https://w.buzzheavier.com/{filename}
    let upload_url = match parent_id {
        Some(pid) if !pid.is_empty() => format!("https://w.buzzheavier.com/{}/{}?locationId=95542dt0et21", pid, urlencoding::encode(filename)),
        _ => format!("https://w.buzzheavier.com/{}?locationId=95542dt0et21", urlencoding::encode(filename)),
    };

    tracing::info!("Buzzheavier upload URL: {}", upload_url);

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

    let body = Body::wrap_stream(stream);

    let response = client
        .put(&upload_url)
        .header("Authorization", format!("Bearer {}", account_id))
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", file_size.to_string())
        .body(body)
        .send()
        .await
        .map_err(|e| Error::Upload(format!("Buzzheavier upload failed (timeout or network error): {}", e)))?;

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
        Error::Upload(format!("Failed to read Buzzheavier response: {}", e))
    })?;

    // Print full response for debugging
    tracing::info!("=== Buzzheavier Response ===");
    tracing::info!("Status: {}", status);
    tracing::info!("Body: {}", body);
    tracing::info!("Upload duration: {:.2}s", duration_secs);
    tracing::info!("Upload speed: {:.2} Mbps ({:.2} MB/s)", speed, speed / 8.0);
    tracing::info!("============================");

    if !status.is_success() {
        return Err(Error::Upload(format!(
            "Buzzheavier upload failed ({}): {}",
            status, body
        )));
    }

    // Try to parse as JSON response
    let result: BuzzheavierResponse = serde_json::from_str(&body).map_err(|e| {
        Error::Upload(format!("Failed to parse Buzzheavier response: {} - body: {}", e, body))
    })?;

    tracing::info!("Parsed Buzzheavier response: {:?}", result);

    // Check for error response
    if let Some(error) = result.error {
        return Err(Error::Upload(format!("Buzzheavier upload failed: {}", error)));
    }
    if let Some(message) = result.message {
        if result.code.map(|c| c >= 400).unwrap_or(false) {
            return Err(Error::Upload(format!("Buzzheavier upload failed: {}", message)));
        }
    }

    // Extract file ID from data object
    let data = result.data.ok_or_else(|| {
        Error::Upload("Buzzheavier response missing data object".to_string())
    })?;

    let file_id = data.id;

    // Construct download URL from file ID
    // Format: https://buzzheavier.com/{id} (NOT /f/{id})
    let download_link = format!("https://buzzheavier.com/{}", file_id);

    tracing::info!("Successfully uploaded to Buzzheavier: {}", download_link);

    Ok(UploadResult {
        download_link,
        file_id,
        upload_speed_mbps: speed,
        duration_secs,
    })
}
