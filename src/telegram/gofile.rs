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

#[derive(Debug, Deserialize)]
pub struct GofileResponse {
    pub status: String,
    pub data: GofileData,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GofileData {
    pub download_page: String,
    pub guest_token: Option<String>,
    pub id: String,
    pub md5: String,
    pub name: String,
    pub parent_folder: String,
    pub parent_folder_code: String,
    pub size: u64,
}

pub struct GofileUploadResult {
    pub download_link: String,
    pub file_code: String,
    pub upload_speed_mbps: f64,
    pub duration_secs: f64,
}

pub async fn upload_to_gofile(
    file_path: &Path,
    filename: &str,
    token: &str,
    progress: Option<SharedUploadProgress>,
) -> Result<GofileUploadResult> {
    let start_time = Instant::now();

    let url = "https://upload.gofile.io/uploadfile";

    let file = File::open(file_path)
        .await
        .map_err(|e| Error::Upload(format!("Failed to open file for gofile upload: {}", e)))?;

    let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

    tracing::info!(
        "Uploading {} ({} bytes / {:.2} MB) to Gofile...",
        filename,
        file_size,
        bytes_to_mb(file_size)
    );

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

    // Build multipart form manually with streaming body
    let form = multipart::Form::new()
        .part("file", multipart::Part::stream_with_length(body, file_size)
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| Error::Upload(format!("Failed to set MIME type: {}", e)))?
        );

    let client = wreq::Client::builder()
        .timeout(Duration::from_secs(60 * 60)) // 1 hour timeout for large files
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| Error::Upload(format!("Failed to create HTTP client: {}", e)))?;

    let boundary = form.boundary().to_string();

    let response = client
        .post(url)
        .bearer_auth(token)
        .multipart(form)
        .header("Content-Type", format!("multipart/form-data; boundary={}", boundary))
        .send()
        .await
        .map_err(|e| Error::Upload(format!("Gofile upload failed: {}", e)))?;

    let duration = start_time.elapsed();
    let duration_secs = duration.as_secs_f64();
    let speed = upload_speed_mbps(file_size, duration_secs);

    // Mark upload as complete with final speed
    if let Some(ref p) = progress {
        let mut prog = p.write().await;
        prog.complete(speed);
    }

    let response_text = response.text().await?;
    let gofile_response: GofileResponse = serde_json::from_str(&response_text)
        .map_err(|e| Error::Upload(format!("Failed to parse gofile response: {} | Response: {}", e, response_text)))?;

    if gofile_response.status != "ok" {
        return Err(Error::Upload(format!(
            "Gofile upload failed with status: {}",
            gofile_response.status
        )));
    }

    let data = gofile_response.data;

    tracing::info!("=== Gofile Upload Complete ===");
    tracing::info!("Upload duration: {:.2}s", duration_secs);
    tracing::info!("Upload speed: {:.2} Mbps ({:.2} MB/s)", speed, speed / 8.0);
    tracing::info!("=================================");

    let result = GofileUploadResult {
        download_link: data.download_page,
        file_code: data.parent_folder_code,
        duration_secs,
        upload_speed_mbps: speed,
    };

    Ok(result)
}
