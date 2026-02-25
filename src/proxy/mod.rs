use crate::config::ProxyConfig;
use crate::error::{Error, Result};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use wreq::Proxy;

/// Geo information from ip-api.com
#[derive(Debug, Deserialize)]
pub struct GeoInfo {
    pub status: String,
    pub country: String,
    #[serde(rename = "countryCode")]
    pub country_code: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default, rename = "regionName")]
    pub region_name: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub isp: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
}

/// SEA (Southeast Asia) country codes + Hong Kong
const SEA_COUNTRIES: &[&str] = &[
    "SG", // Singapore
    "MY", // Malaysia
    "TH", // Thailand
    "VN", // Vietnam
    "ID", // Indonesia
    "PH", // Philippines
    "MM", // Myanmar
    "KH", // Cambodia
    "LA", // Laos
    "BN", // Brunei
    "TL", // Timor-Leste
    "HK", // Hong Kong
];

/// Proxy region type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyRegion {
    /// No proxy, direct connection
    Direct,
    /// SEA region proxy
    Sea,
    /// US region proxy
    Us,
}

/// Proxy manager handles geo detection and proxy selection
pub struct ProxyManager {
    config: ProxyConfig,
    detected_region: Arc<RwLock<Option<DetectedRegion>>>,
}

#[derive(Debug, Clone)]
struct DetectedRegion {
    country_code: String,
    is_sea: bool,
    is_us: bool,
}

impl ProxyManager {
    pub fn new(config: ProxyConfig) -> Self {
        Self {
            config,
            detected_region: Arc::new(RwLock::new(None)),
        }
    }

    /// Detect the hosting server's geographic location
    pub async fn detect_location(&self) -> Result<()> {
        let client = wreq::Client::new();

        let response = client
            .get("http://ip-api.com/json/")
            .send()
            .await
            .map_err(|e| Error::Network(format!("Failed to detect geo location: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Network(format!(
                "Geo detection failed with status: {}",
                response.status()
            )));
        }

        let geo: GeoInfo = response.json().await.map_err(|e| {
            Error::Network(format!("Failed to parse geo response: {}", e))
        })?;

        if geo.status != "success" {
            return Err(Error::Network("Geo detection returned non-success status".to_string()));
        }

        let country_code = geo.country_code.to_uppercase();
        let is_sea = SEA_COUNTRIES.contains(&country_code.as_str());
        let is_us = country_code == "US";

        tracing::info!(
            "Detected hosting location: {} ({}) [{}] - SEA: {}, US: {}",
            geo.country,
            country_code,
            geo.query.as_deref().unwrap_or("unknown IP"),
            is_sea,
            is_us
        );

        *self.detected_region.write().await = Some(DetectedRegion {
            country_code,
            is_sea,
            is_us,
        });

        Ok(())
    }

    /// Check if we're in SEA region
    pub async fn is_in_sea(&self) -> bool {
        self.detected_region
            .read()
            .await
            .as_ref()
            .map(|r| r.is_sea)
            .unwrap_or(false)
    }

    /// Check if we're in US region
    pub async fn is_in_us(&self) -> bool {
        self.detected_region
            .read()
            .await
            .as_ref()
            .map(|r| r.is_us)
            .unwrap_or(false)
    }

    /// Get the detected country code
    pub async fn get_country_code(&self) -> Option<String> {
        self.detected_region
            .read()
            .await
            .as_ref()
            .map(|r| r.country_code.clone())
    }

    /// Get the appropriate proxy for general API requests
    /// Uses SEA proxy if not in SEA region
    pub async fn get_default_proxy(&self) -> Option<String> {
        if self.is_in_sea().await {
            None // Direct connection if already in SEA
        } else {
            self.config.sea_proxy.clone()
        }
    }

    /// Get the US proxy for search/browse requests
    /// Only returns proxy if not already in US
    pub async fn get_us_proxy(&self) -> Option<String> {
        if self.is_in_us().await {
            None // Direct connection if already in US
        } else {
            self.config.us_proxy.clone()
        }
    }

    /// Check if US proxy is available
    pub fn has_us_proxy(&self) -> bool {
        self.config.us_proxy.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
    }

    /// Check if SEA proxy is available
    pub fn has_sea_proxy(&self) -> bool {
        self.config.sea_proxy.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
    }

    /// Create a wreq Proxy from a proxy URL string
    pub fn parse_proxy(proxy_url: &str) -> Result<Proxy> {
        Proxy::all(proxy_url).map_err(|e| {
            Error::Config(format!("Invalid proxy URL '{}': {}", proxy_url, e))
        })
    }

    /// Build an HTTP client with optional proxy
    pub fn build_client_with_proxy(proxy_url: Option<&str>) -> Result<wreq::Client> {
        let mut builder = wreq::Client::builder()
            .cookie_store(true)
            .redirect(wreq::redirect::Policy::limited(10));

        if let Some(url) = proxy_url {
            if !url.is_empty() {
                let proxy = Self::parse_proxy(url)?;
                builder = builder.proxy(proxy);
                tracing::debug!("Using proxy: {}", url);
            }
        }

        builder
            .build()
            .map_err(|e| Error::Network(format!("Failed to build HTTP client: {}", e)))
    }
}
