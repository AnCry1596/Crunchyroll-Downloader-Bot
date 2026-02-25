use crate::crunchyroll::endpoints::{api_url, AUTH_TOKEN, BASIC_AUTH, DEVICE_NAME, DEVICE_TYPE, USER_AGENT};
use crate::crunchyroll::types::{AuthSession, TokenResponse};
use crate::error::{Error, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub struct AuthManager {
    http: wreq::Client,
    session: Arc<RwLock<Option<AuthSession>>>,
    email: String,
    password: String,
}

impl AuthManager {
    pub fn new(http: wreq::Client, email: String, password: String) -> Self {
        Self {
            http,
            session: Arc::new(RwLock::new(None)),
            email,
            password,
        }
    }

    /// Perform login with email/password
    pub async fn login(&self) -> Result<()> {
        let device_id = Uuid::new_v4().to_string();

        let response = self
            .http
            .post(api_url(AUTH_TOKEN))
            .header("Authorization", format!("Basic {}", BASIC_AUTH))
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("username", self.email.as_str()),
                ("password", self.password.as_str()),
                ("grant_type", "password"),
                ("scope", "offline_access"),
                ("device_id", &device_id),
                ("device_name", DEVICE_NAME),
                ("device_type", DEVICE_TYPE),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            if body.contains("invalid_credentials") {
                return Err(Error::InvalidCredentials);
            }

            return Err(Error::Auth(format!(
                "Login failed with status {}: {}",
                status, body
            )));
        }

        let token: TokenResponse = response.json().await.map_err(|e| {
            Error::Auth(format!("Failed to parse token response: {}", e))
        })?;

        let session = AuthSession::new(token, device_id);
        *self.session.write().await = Some(session);

        tracing::info!("Successfully logged in to Crunchyroll");
        Ok(())
    }

    /// Perform anonymous login (no credentials, limited access)
    pub async fn login_anonymous(&self) -> Result<()> {
        let device_id = Uuid::new_v4().to_string();

        let response = self
            .http
            .post(api_url(AUTH_TOKEN))
            .header("Authorization", format!("Basic {}", BASIC_AUTH))
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "client_id"),
                ("scope", "offline_access"),
                ("device_id", &device_id),
                ("device_name", DEVICE_NAME),
                ("device_type", DEVICE_TYPE),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Auth(format!(
                "Anonymous login failed with status {}: {}",
                status, body
            )));
        }

        let token: TokenResponse = response.json().await?;
        let session = AuthSession::new(token, device_id);
        *self.session.write().await = Some(session);

        tracing::info!("Anonymous login successful");
        Ok(())
    }

    /// Refresh the access token using refresh_token
    pub async fn refresh_token(&self) -> Result<()> {
        let (refresh_token, device_id) = {
            let session = self.session.read().await;
            let session = session.as_ref().ok_or(Error::TokenExpired)?;

            let refresh_token = session
                .token
                .refresh_token
                .clone()
                .ok_or(Error::TokenExpired)?;

            (refresh_token, session.device_id.clone())
        };

        let response = self
            .http
            .post(api_url(AUTH_TOKEN))
            .header("Authorization", format!("Basic {}", BASIC_AUTH))
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("refresh_token", refresh_token.as_str()),
                ("grant_type", "refresh_token"),
                ("scope", "offline_access"),
                ("device_id", &device_id),
                ("device_name", DEVICE_NAME),
                ("device_type", DEVICE_TYPE),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            tracing::warn!("Token refresh failed, attempting full re-login");
            return self.login().await;
        }

        let token: TokenResponse = response.json().await?;
        let session = AuthSession::new(token, device_id);
        *self.session.write().await = Some(session);

        tracing::debug!("Token refreshed successfully");
        Ok(())
    }

    /// Ensure we have a valid token, refreshing if needed
    pub async fn ensure_valid_token(&self) -> Result<String> {
        // Check if we have a session and if it needs refresh
        {
            let session = self.session.read().await;
            if let Some(ref s) = *session {
                if !s.needs_refresh() {
                    return Ok(s.access_token().to_string());
                }
            }
        }

        // Need to refresh or login
        {
            let session = self.session.read().await;
            if session.is_some() {
                drop(session);
                self.refresh_token().await?;
            } else {
                drop(session);
                self.login().await?;
            }
        }

        // Get the new token
        let session = self.session.read().await;
        session
            .as_ref()
            .map(|s| s.access_token().to_string())
            .ok_or(Error::TokenExpired)
    }

    /// Get the current session (if any)
    pub async fn get_session(&self) -> Option<AuthSession> {
        self.session.read().await.clone()
    }

    /// Check if we have a valid session
    pub async fn is_authenticated(&self) -> bool {
        let session = self.session.read().await;
        session.as_ref().map(|s| !s.is_expired()).unwrap_or(false)
    }

    /// Get the account ID from the current session
    pub async fn get_account_id(&self) -> Option<String> {
        let session = self.session.read().await;
        session.as_ref().map(|s| s.token.account_id.clone())
    }

    /// Get the country code from the current session
    pub async fn get_country(&self) -> Option<String> {
        let session = self.session.read().await;
        session.as_ref().map(|s| s.token.country.clone())
    }

    /// Force refresh the token (used when getting 403 errors)
    /// This will try refresh_token first, then fall back to full re-login
    pub async fn force_refresh(&self) -> Result<String> {
        tracing::info!("Forcing token refresh due to 403 error");

        // Try refresh token first
        if let Err(e) = self.refresh_token().await {
            tracing::warn!("Token refresh failed: {}, attempting full re-login", e);
            self.login().await?;
        }

        // Get the new token
        let session = self.session.read().await;
        session
            .as_ref()
            .map(|s| s.access_token().to_string())
            .ok_or(Error::TokenExpired)
    }
}
