use crate::config::WidevineConfig;
use crate::crunchyroll::endpoints::LICENSE_WIDEVINE;
use crate::error::{Error, Result};
use base64::Engine;
use rsa::pkcs8::DecodePrivateKey;
use rsa::RsaPrivateKey;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use widevine::device::{Device, DeviceType, SecurityLevel};
use widevine::{Cdm, LicenseType, Pssh};

/// License server JSON response structure
#[derive(Debug, Deserialize)]
struct LicenseResponse {
    status: String,
    license: String,
}

/// Content decryption key
#[derive(Debug, Clone)]
pub struct ContentKey {
    /// Key ID in hex format
    pub kid: String,
    /// Decryption key in hex format
    pub key: String,
    /// Key type
    pub key_type: KeyType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyType {
    Content,
    Signing,
    Unknown,
}

impl ContentKey {
    /// Format for mp4decrypt: kid:key
    pub fn to_mp4decrypt_arg(&self) -> String {
        format!("{}:{}", self.kid, self.key)
    }
}

/// Widevine CDM wrapper for license acquisition
pub struct WidevineCdm {
    device: Device,
    /// Cache of decryption keys by PSSH
    key_cache: RwLock<HashMap<String, Vec<ContentKey>>>,
}

impl WidevineCdm {
    /// Create a new CDM from device files
    pub fn new(config: &WidevineConfig) -> Result<Self> {
        let device = Self::load_device(&config.client_id_path, &config.private_key_path)?;
        Ok(Self {
            device,
            key_cache: RwLock::new(HashMap::new()),
        })
    }

    /// Load device from a WVD file (preferred method)
    pub fn from_wvd(wvd_path: &Path) -> Result<Self> {
        let file = std::fs::File::open(wvd_path).map_err(|e| {
            Error::DeviceCredentials(format!("Không thể mở file WVD {}: {}", wvd_path.display(), e))
        })?;

        let device = Device::read_wvd(file).map_err(|e| {
            Error::DeviceCredentials(format!("Failed to read WVD file: {}", e))
        })?;

        Ok(Self {
            device,
            key_cache: RwLock::new(HashMap::new()),
        })
    }

    /// Check if keys are cached for a given PSSH
    fn get_cached_keys(&self, pssh_b64: &str) -> Option<Vec<ContentKey>> {
        let cache = self.key_cache.read().ok()?;
        cache.get(pssh_b64).cloned()
    }

    /// Cache keys for a given PSSH
    fn cache_keys(&self, pssh_b64: &str, keys: &[ContentKey]) {
        if let Ok(mut cache) = self.key_cache.write() {
            cache.insert(pssh_b64.to_string(), keys.to_vec());
            tracing::debug!("Cached {} keys for PSSH", keys.len());
        }
    }

    /// Load device from separate client_id and private_key files
    fn load_device(client_id_path: &Path, private_key_path: &Path) -> Result<Device> {
        // Read client ID blob
        let client_id = std::fs::read(client_id_path).map_err(|e| {
            Error::DeviceCredentials(format!(
                "Không thể đọc client_id từ {}: {}",
                client_id_path.display(), e
            ))
        })?;

        // Read private key PEM
        let private_key_pem = std::fs::read_to_string(private_key_path).map_err(|e| {
            Error::DeviceCredentials(format!(
                "Không thể đọc private_key từ {}: {}",
                private_key_path.display(), e
            ))
        })?;

        // Parse RSA private key from PEM
        let private_key = RsaPrivateKey::from_pkcs8_pem(&private_key_pem).map_err(|e| {
            Error::DeviceCredentials(format!("Failed to parse private key: {}", e))
        })?;

        // Create device with typical Android settings
        Device::new(
            DeviceType::ANDROID,
            SecurityLevel::L3,
            private_key,
            &client_id,
        )
        .map_err(|e| Error::DeviceCredentials(format!("Failed to create Widevine device: {}", e)))
    }

    /// Request content decryption keys from license server
    pub async fn get_keys(
        &self,
        pssh_b64: &str,
        license_url: Option<&str>,
        _http: &wreq::Client,
        auth_token: &str,
        content_id: Option<&str>,
        video_token: Option<&str>,
    ) -> Result<Vec<ContentKey>> {
        // Check cache first
        if let Some(cached_keys) = self.get_cached_keys(pssh_b64) {
            tracing::info!("Using cached decryption keys ({} keys)", cached_keys.len());
            return Ok(cached_keys);
        }

        // Parse PSSH from base64
        let pssh = Pssh::from_b64(pssh_b64)
            .map_err(|e| Error::Widevine(format!("Failed to parse PSSH: {}", e)))?;

        // Create CDM session
        let cdm = Cdm::new(self.device.clone());
        let session = cdm.open();

        // Generate license request (returns CdmLicenseRequest)
        let license_request = session
            .get_license_request(pssh, LicenseType::STREAMING)
            .map_err(|e| Error::Widevine(format!("Failed to generate license request: {}", e)))?;

        // Get the challenge bytes from the request
        let challenge = license_request
            .challenge()
            .map_err(|e| Error::Widevine(format!("Failed to get challenge: {}", e)))?;

        // Use provided license URL or default Crunchyroll URL
        let url = license_url.unwrap_or(LICENSE_WIDEVINE);

        tracing::debug!("Requesting license from: {}", url);
        tracing::debug!("Content ID: {:?}, Video Token: {:?}", content_id, video_token);

        // Create a new client with browser emulation for the license request
        let license_client = wreq::Client::builder()
            .redirect(wreq::redirect::Policy::limited(10))
            .emulation(wreq_util::Emulation::Chrome136)
            .build()
            .map_err(|e| Error::License(format!("Failed to create license client: {}", e)))?;

        // Build request with browser-like headers
        // Using static.crunchyroll.com as origin/referer like the web player
        let mut request = license_client
            .post(url)
            .bearer_auth(auth_token)
            .header("Content-Type", "application/octet-stream")
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Origin", "https://static.crunchyroll.com")
            .header("Referer", "https://static.crunchyroll.com/")
            .header("Cache-Control", "no-cache")
            .header("Pragma", "no-cache");

        // Add Crunchyroll-specific headers if provided
        if let Some(cid) = content_id {
            request = request.header("X-Cr-Content-Id", cid);
        }
        if let Some(token) = video_token {
            request = request.header("X-Cr-Video-Token", token);
        }

        let response = request
            .body(challenge)
            .send()
            .await
            .map_err(|e| Error::License(format!("License request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::License(format!(
                "License server returned {}: {}",
                status, body
            )));
        }

        // Parse the JSON response from the license server
        let response_text = response.text().await.map_err(|e| {
            Error::License(format!("Failed to read license response: {}", e))
        })?;

        tracing::debug!("License response: {}", &response_text[..response_text.len().min(500)]);

        let license_response: LicenseResponse = serde_json::from_str(&response_text)
            .map_err(|e| Error::License(format!("Failed to parse license JSON: {}", e)))?;

        if license_response.status != "OK" {
            return Err(Error::License(format!(
                "License server returned status: {}",
                license_response.status
            )));
        }

        // Decode the base64-encoded license data
        let license_data = base64::engine::general_purpose::STANDARD
            .decode(&license_response.license)
            .map_err(|e| Error::License(format!("Failed to decode license base64: {}", e)))?;

        tracing::debug!("Decoded license data: {} bytes", license_data.len());

        // Parse license response and extract keys from the license_request
        let key_set = license_request
            .get_keys(&license_data)
            .map_err(|e| Error::Widevine(format!("Failed to parse license: {}", e)))?;

        // Convert keys to our ContentKey format
        let mut content_keys: Vec<ContentKey> = Vec::new();

        // Get content keys (type CONTENT)
        for k in key_set.of_type(widevine::KeyType::CONTENT) {
            content_keys.push(ContentKey {
                kid: hex::encode(&k.kid),
                key: hex::encode(&k.key),
                key_type: KeyType::Content,
            });
        }

        // Get signing keys
        for k in key_set.of_type(widevine::KeyType::SIGNING) {
            content_keys.push(ContentKey {
                kid: hex::encode(&k.kid),
                key: hex::encode(&k.key),
                key_type: KeyType::Signing,
            });
        }

        if content_keys.is_empty() {
            return Err(Error::NoContentKeys);
        }

        tracing::info!("Got {} content keys from license server", content_keys.len());

        // Cache the keys for future use
        self.cache_keys(pssh_b64, &content_keys);

        Ok(content_keys)
    }

    /// Get content keys only (filter out signing/other keys)
    pub async fn get_content_keys(
        &self,
        pssh_b64: &str,
        license_url: Option<&str>,
        http: &wreq::Client,
        auth_token: &str,
        content_id: Option<&str>,
        video_token: Option<&str>,
    ) -> Result<Vec<ContentKey>> {
        let all_keys = self
            .get_keys(pssh_b64, license_url, http, auth_token, content_id, video_token)
            .await?;

        let content_keys: Vec<ContentKey> = all_keys
            .into_iter()
            .filter(|k| k.key_type == KeyType::Content)
            .collect();

        if content_keys.is_empty() {
            return Err(Error::NoContentKeys);
        }

        Ok(content_keys)
    }
}

/// Extract PSSH from MPD manifest content
pub fn extract_pssh_from_mpd(mpd_content: &str) -> Option<String> {
    // Look for Widevine PSSH in ContentProtection element
    // Format: <cenc:pssh>BASE64_PSSH</cenc:pssh>
    // or: <pssh>BASE64_PSSH</pssh>

    let pssh_patterns = [
        ("<cenc:pssh>", "</cenc:pssh>"),
        ("<pssh>", "</pssh>"),
    ];

    for (start_tag, end_tag) in &pssh_patterns {
        if let Some(start) = mpd_content.find(start_tag) {
            let pssh_start = start + start_tag.len();
            if let Some(end) = mpd_content[pssh_start..].find(end_tag) {
                let pssh = &mpd_content[pssh_start..pssh_start + end];
                // Verify it's base64 and not empty
                if !pssh.is_empty()
                    && pssh
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '=')
                {
                    return Some(pssh.to_string());
                }
            }
        }
    }

    None
}
