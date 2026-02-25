use crate::database::models::{ActiveDownload, AdminUser, AuthorizedChat, CachedBuzzheavierFile, CachedGofileFile, CachedFile, CachedKey, CachedPixeldrainFile, DownloadRequest, RequestStatus};
use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use mongodb::{
    bson::{doc, DateTime as BsonDateTime},
    options::{ClientOptions, IndexOptions, UpdateOptions},
    Client, Collection, IndexModel,
};

/// Convert chrono DateTime to BSON DateTime
fn to_bson_datetime(dt: DateTime<Utc>) -> BsonDateTime {
    BsonDateTime::from_millis(dt.timestamp_millis())
}

/// Database client for MongoDB operations
#[derive(Clone)]
pub struct Database {
    client: Client,
    db_name: String,
}

impl Database {
    /// Connect to MongoDB
    pub async fn connect(connection_string: &str, db_name: &str) -> Result<Self> {
        let client_options = ClientOptions::parse(connection_string)
            .await
            .map_err(|e| Error::Database(format!("Failed to parse MongoDB connection string: {}", e)))?;

        let client = Client::with_options(client_options)
            .map_err(|e| Error::Database(format!("Failed to create MongoDB client: {}", e)))?;

        // Test connection
        client
            .database(db_name)
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| Error::Database(format!("Failed to connect to MongoDB: {}", e)))?;

        let db = Self {
            client,
            db_name: db_name.to_string(),
        };

        // Create indexes
        db.create_indexes().await?;

        tracing::info!("Connected to MongoDB database: {}", db_name);
        Ok(db)
    }

    /// Create necessary indexes
    async fn create_indexes(&self) -> Result<()> {
        // Index for cached files by content_id (already _id, so no need)

        // Index for cached keys by content_id
        let keys_collection = self.keys_collection();
        let content_id_index = IndexModel::builder()
            .keys(doc! { "content_id": 1 })
            .options(IndexOptions::builder().sparse(true).build())
            .build();
        keys_collection
            .create_index(content_id_index)
            .await
            .map_err(|e| Error::Database(format!("Failed to create index: {}", e)))?;

        // Indexes for download requests
        let requests_collection = self.requests_collection();

        let user_id_index = IndexModel::builder()
            .keys(doc! { "user_id": 1 })
            .build();
        requests_collection
            .create_index(user_id_index)
            .await
            .map_err(|e| Error::Database(format!("Failed to create index: {}", e)))?;

        let content_id_index = IndexModel::builder()
            .keys(doc! { "content_id": 1 })
            .build();
        requests_collection
            .create_index(content_id_index)
            .await
            .map_err(|e| Error::Database(format!("Failed to create index: {}", e)))?;

        let requested_at_index = IndexModel::builder()
            .keys(doc! { "requested_at": -1 })
            .build();
        requests_collection
            .create_index(requested_at_index)
            .await
            .map_err(|e| Error::Database(format!("Failed to create index: {}", e)))?;

        Ok(())
    }

    fn files_collection(&self) -> Collection<CachedFile> {
        self.client.database(&self.db_name).collection("cached_files")
    }

    fn keys_collection(&self) -> Collection<CachedKey> {
        self.client.database(&self.db_name).collection("cached_keys")
    }

    fn requests_collection(&self) -> Collection<DownloadRequest> {
        self.client.database(&self.db_name).collection("requests")
    }

    fn pixeldrain_collection(&self) -> Collection<CachedPixeldrainFile> {
        self.client.database(&self.db_name).collection("pixeldrain_cache")
    }

    fn buzzheavier_collection(&self) -> Collection<CachedBuzzheavierFile> {
        self.client.database(&self.db_name).collection("buzzheavier_cache")
    }

    fn active_downloads_collection(&self) -> Collection<ActiveDownload> {
        self.client.database(&self.db_name).collection("active_downloads")
    }

    fn gofile_collection(&self) -> Collection<CachedGofileFile> {
        self.client.database(&self.db_name).collection("gofile_cache")
    }

    fn admins_collection(&self) -> Collection<AdminUser> {
        self.client.database(&self.db_name).collection("admins")
    }

    fn authorized_chats_collection(&self) -> Collection<AuthorizedChat> {
        self.client.database(&self.db_name).collection("authorized_chats")
    }

    // ==================== Admins ====================

    /// Check if a user is an admin
    pub async fn is_admin(&self, user_id: i64) -> Result<bool> {
        let result = self.admins_collection()
            .find_one(doc! { "_id": user_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to check admin: {}", e)))?;
        Ok(result.is_some())
    }

    /// Add an admin user
    pub async fn add_admin(&self, admin: &AdminUser) -> Result<()> {
        self.admins_collection()
            .insert_one(admin)
            .await
            .map_err(|e| Error::Database(format!("Failed to add admin: {}", e)))?;
        Ok(())
    }

    /// Remove an admin user
    pub async fn remove_admin(&self, user_id: i64) -> Result<bool> {
        let result = self.admins_collection()
            .delete_one(doc! { "_id": user_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to remove admin: {}", e)))?;
        Ok(result.deleted_count > 0)
    }

    /// Get all admin users
    pub async fn get_all_admins(&self) -> Result<Vec<AdminUser>> {
        use futures::TryStreamExt;
        let cursor = self.admins_collection()
            .find(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to get admins: {}", e)))?;
        let admins: Vec<AdminUser> = cursor
            .try_collect()
            .await
            .map_err(|e| Error::Database(format!("Failed to collect admins: {}", e)))?;
        Ok(admins)
    }

    // ==================== Authorized Chats ====================

    /// Check if a chat is authorized
    pub async fn is_chat_authorized(&self, chat_id: i64) -> Result<bool> {
        let result = self.authorized_chats_collection()
            .find_one(doc! { "_id": chat_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to check authorized chat: {}", e)))?;
        Ok(result.is_some())
    }

    /// Authorize a chat
    pub async fn authorize_chat(&self, chat: &AuthorizedChat) -> Result<()> {
        self.authorized_chats_collection()
            .insert_one(chat)
            .await
            .map_err(|e| Error::Database(format!("Failed to authorize chat: {}", e)))?;
        Ok(())
    }

    /// Remove chat authorization
    pub async fn deauthorize_chat(&self, chat_id: i64) -> Result<bool> {
        let result = self.authorized_chats_collection()
            .delete_one(doc! { "_id": chat_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to deauthorize chat: {}", e)))?;
        Ok(result.deleted_count > 0)
    }

    /// Get all authorized chats
    pub async fn get_all_authorized_chats(&self) -> Result<Vec<AuthorizedChat>> {
        use futures::TryStreamExt;
        let cursor = self.authorized_chats_collection()
            .find(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to get authorized chats: {}", e)))?;
        let chats: Vec<AuthorizedChat> = cursor
            .try_collect()
            .await
            .map_err(|e| Error::Database(format!("Failed to collect authorized chats: {}", e)))?;
        Ok(chats)
    }

    // ==================== Cached Files ====================

    /// Get a cached file by content ID
    pub async fn get_cached_file(&self, content_id: &str) -> Result<Option<CachedFile>> {
        self.files_collection()
            .find_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to get cached file: {}", e)))
    }

    /// Save a cached file
    pub async fn save_cached_file(&self, file: &CachedFile) -> Result<()> {
        let options = UpdateOptions::builder().upsert(true).build();
        let update_doc = doc! {
            "$set": {
                "file_id": &file.file_id,
                "filename": &file.filename,
                "file_size": file.file_size as i64,
                "resolution": &file.resolution,
                "bitrate": file.bitrate.map(|b| b as i64),
                "series_title": &file.series_title,
                "season_title": &file.season_title,
                "episode_number": &file.episode_number,
                "episode_title": &file.episode_title,
                "audio_locale": &file.audio_locale,
                "subtitle_locales": &file.subtitle_locales,
                "message_id": file.message_id,
                "storage_chat_id": file.storage_chat_id,
                "cached_at": to_bson_datetime(file.cached_at),
            },
            "$setOnInsert": {
                "forward_count": 0_i32
            }
        };

        self.files_collection()
            .update_one(doc! { "_id": &file.content_id }, update_doc)
            .with_options(options)
            .await
            .map_err(|e| Error::Database(format!("Failed to save cached file: {}", e)))?;

        Ok(())
    }

    /// Increment the forward count for a cached file
    pub async fn increment_forward_count(&self, content_id: &str) -> Result<()> {
        self.files_collection()
            .update_one(
                doc! { "_id": content_id },
                doc! { "$inc": { "forward_count": 1 } },
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to increment forward count: {}", e)))?;

        Ok(())
    }

    /// Delete a cached Telegram file (for subtitle invalidation)
    pub async fn delete_cached_file(&self, content_id: &str) -> Result<()> {
        self.files_collection()
            .delete_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to delete cached file: {}", e)))?;

        Ok(())
    }

    // ==================== Cached Keys ====================

    /// Get cached keys by PSSH
    pub async fn get_cached_keys(&self, pssh: &str) -> Result<Option<CachedKey>> {
        self.keys_collection()
            .find_one(doc! { "_id": pssh })
            .await
            .map_err(|e| Error::Database(format!("Failed to get cached keys: {}", e)))
    }

    /// Get cached keys by content ID
    pub async fn get_cached_keys_by_content(&self, content_id: &str) -> Result<Option<CachedKey>> {
        self.keys_collection()
            .find_one(doc! { "content_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to get cached keys: {}", e)))
    }

    /// Save cached keys
    pub async fn save_cached_keys(&self, keys: &CachedKey) -> Result<()> {
        let options = UpdateOptions::builder().upsert(true).build();

        let keys_bson: Vec<_> = keys
            .keys
            .iter()
            .map(|k| doc! { "kid": &k.kid, "key": &k.key })
            .collect();

        let update_doc = doc! {
            "$set": {
                "content_id": &keys.content_id,
                "keys": keys_bson,
                "fetched_at": to_bson_datetime(keys.fetched_at),
            },
            "$setOnInsert": {
                "use_count": 0_i32
            }
        };

        self.keys_collection()
            .update_one(doc! { "_id": &keys.pssh }, update_doc)
            .with_options(options)
            .await
            .map_err(|e| Error::Database(format!("Failed to save cached keys: {}", e)))?;

        Ok(())
    }

    /// Increment the use count for cached keys
    pub async fn increment_key_use_count(&self, pssh: &str) -> Result<()> {
        self.keys_collection()
            .update_one(
                doc! { "_id": pssh },
                doc! { "$inc": { "use_count": 1 } },
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to increment key use count: {}", e)))?;

        Ok(())
    }

    // ==================== Pixeldrain Cache ====================

    /// Get a cached Pixeldrain file by content ID (if not expired)
    pub async fn get_cached_pixeldrain(&self, content_id: &str) -> Result<Option<CachedPixeldrainFile>> {
        let file = self.pixeldrain_collection()
            .find_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to get cached pixeldrain file: {}", e)))?;

        // Check if the cache is still valid
        if let Some(ref f) = file {
            if !f.is_valid() {
                // Cache expired, delete it
                let _ = self.delete_cached_pixeldrain(content_id).await;
                return Ok(None);
            }
        }

        Ok(file)
    }

    /// Save a cached Pixeldrain file
    pub async fn save_cached_pixeldrain(&self, file: &CachedPixeldrainFile) -> Result<()> {
        let options = UpdateOptions::builder().upsert(true).build();

        let keys_bson: Vec<_> = file
            .decryption_keys
            .iter()
            .map(|k| doc! { "kid": &k.kid, "key": &k.key })
            .collect();

        let update_doc = doc! {
            "$set": {
                "pixeldrain_id": &file.pixeldrain_id,
                "download_url": &file.download_url,
                "filename": &file.filename,
                "file_size": file.file_size as i64,
                "series_title": &file.series_title,
                "episode_number": &file.episode_number,
                "episode_title": &file.episode_title,
                "audio_locale": &file.audio_locale,
                "subtitle_locales": &file.subtitle_locales,
                "decryption_keys": keys_bson,
                "cached_at": to_bson_datetime(file.cached_at),
                "expires_at": to_bson_datetime(file.expires_at),
            },
            "$setOnInsert": {
                "serve_count": 1_i32
            }
        };

        self.pixeldrain_collection()
            .update_one(doc! { "_id": &file.content_id }, update_doc)
            .with_options(options)
            .await
            .map_err(|e| Error::Database(format!("Failed to save cached pixeldrain file: {}", e)))?;

        Ok(())
    }

    /// Increment the serve count for a cached Pixeldrain file
    pub async fn increment_pixeldrain_serve_count(&self, content_id: &str) -> Result<()> {
        self.pixeldrain_collection()
            .update_one(
                doc! { "_id": content_id },
                doc! { "$inc": { "serve_count": 1 } },
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to increment pixeldrain serve count: {}", e)))?;

        Ok(())
    }

    /// Delete a cached Pixeldrain file
    pub async fn delete_cached_pixeldrain(&self, content_id: &str) -> Result<()> {
        self.pixeldrain_collection()
            .delete_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to delete cached pixeldrain file: {}", e)))?;

        Ok(())
    }

    /// Clean up expired Pixeldrain cache entries
    pub async fn cleanup_expired_pixeldrain(&self) -> Result<u64> {
        let now = BsonDateTime::now();

        let result = self.pixeldrain_collection()
            .delete_many(doc! { "expires_at": { "$lt": now } })
            .await
            .map_err(|e| Error::Database(format!("Failed to cleanup expired pixeldrain cache: {}", e)))?;

        Ok(result.deleted_count)
    }

    // ==================== Buzzheavier Cache ====================

    /// Get a cached Buzzheavier file by content ID (if not expired)
    pub async fn get_cached_buzzheavier(&self, content_id: &str) -> Result<Option<CachedBuzzheavierFile>> {
        let file = self.buzzheavier_collection()
            .find_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to get cached buzzheavier file: {}", e)))?;

        // Check if the cache is still valid
        if let Some(ref f) = file {
            if !f.is_valid() {
                // Cache expired, delete it
                let _ = self.delete_cached_buzzheavier(content_id).await;
                return Ok(None);
            }
        }

        Ok(file)
    }

    /// Save a cached Buzzheavier file
    pub async fn save_cached_buzzheavier(&self, file: &CachedBuzzheavierFile) -> Result<()> {
        let options = UpdateOptions::builder().upsert(true).build();

        let keys_bson: Vec<_> = file
            .decryption_keys
            .iter()
            .map(|k| doc! { "kid": &k.kid, "key": &k.key })
            .collect();

        let update_doc = doc! {
            "$set": {
                "buzzheavier_id": &file.buzzheavier_id,
                "download_url": &file.download_url,
                "filename": &file.filename,
                "file_size": file.file_size as i64,
                "series_title": &file.series_title,
                "episode_number": &file.episode_number,
                "episode_title": &file.episode_title,
                "audio_locale": &file.audio_locale,
                "subtitle_locales": &file.subtitle_locales,
                "decryption_keys": keys_bson,
                "cached_at": to_bson_datetime(file.cached_at),
                "expires_at": to_bson_datetime(file.expires_at),
            },
            "$setOnInsert": {
                "serve_count": 1_i32
            }
        };

        self.buzzheavier_collection()
            .update_one(doc! { "_id": &file.content_id }, update_doc)
            .with_options(options)
            .await
            .map_err(|e| Error::Database(format!("Failed to save cached buzzheavier file: {}", e)))?;

        Ok(())
    }

    /// Increment the serve count for a cached Buzzheavier file
    pub async fn increment_buzzheavier_serve_count(&self, content_id: &str) -> Result<()> {
        self.buzzheavier_collection()
            .update_one(
                doc! { "_id": content_id },
                doc! { "$inc": { "serve_count": 1 } },
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to increment buzzheavier serve count: {}", e)))?;

        Ok(())
    }

    /// Delete a cached Buzzheavier file
    pub async fn delete_cached_buzzheavier(&self, content_id: &str) -> Result<()> {
        self.buzzheavier_collection()
            .delete_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to delete cached buzzheavier file: {}", e)))?;

        Ok(())
    }

    /// Clean up expired Buzzheavier cache entries
    pub async fn cleanup_expired_buzzheavier(&self) -> Result<u64> {
        let now = BsonDateTime::now();

        let result = self.buzzheavier_collection()
            .delete_many(doc! { "expires_at": { "$lt": now } })
            .await
            .map_err(|e| Error::Database(format!("Failed to cleanup expired buzzheavier cache: {}", e)))?;

        Ok(result.deleted_count)
    }

    // ==================== Gofile Cache ====================

    /// Get a cached Gofile file by content ID (if not expired)
    pub async fn get_cached_gofile(&self, content_id: &str) -> Result<Option<CachedGofileFile>> {
        let file = self.gofile_collection()
            .find_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to get cached gofile file: {}", e)))?;

        // Check if the cache is still valid
        if let Some(ref f) = file {
            if !f.is_valid() {
                // Cache expired, delete it
                let _ = self.delete_cached_gofile(content_id).await;
                return Ok(None);
            }
        }

        Ok(file)
    }

    /// Save a cached Gofile file
    pub async fn save_cached_gofile(&self, file: &CachedGofileFile) -> Result<()> {
        let options = UpdateOptions::builder().upsert(true).build();

        let keys_bson: Vec<_> = file
            .decryption_keys
            .iter()
            .map(|k| doc! { "kid": &k.kid, "key": &k.key })
            .collect();

        let update_doc = doc! {
            "$set": {
                "gofile_file_code": &file.gofile_file_code,
                "download_url": &file.download_url,
                "filename": &file.filename,
                "file_size": file.file_size as i64,
                "series_title": &file.series_title,
                "episode_number": &file.episode_number,
                "episode_title": &file.episode_title,
                "audio_locale": &file.audio_locale,
                "subtitle_locales": &file.subtitle_locales,
                "decryption_keys": keys_bson,
                "cached_at": to_bson_datetime(file.cached_at),
                "expires_at": to_bson_datetime(file.expires_at),
            },
            "$setOnInsert": {
                "serve_count": 1_i32
            }
        };

        self.gofile_collection()
            .update_one(doc! { "_id": &file.content_id }, update_doc)
            .with_options(options)
            .await
            .map_err(|e| Error::Database(format!("Failed to save cached gofile file: {}", e)))?;

        Ok(())
    }

    /// Increment the serve count for a cached Gofile file
    pub async fn increment_gofile_serve_count(&self, content_id: &str) -> Result<()> {
        self.gofile_collection()
            .update_one(
                doc! { "_id": content_id },
                doc! { "$inc": { "serve_count": 1 } },
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to increment gofile serve count: {}", e)))?;

        Ok(())
    }

    /// Delete a cached Gofile file
    pub async fn delete_cached_gofile(&self, content_id: &str) -> Result<()> {
        self.gofile_collection()
            .delete_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to delete cached gofile file: {}", e)))?;

        Ok(())
    }

    // ==================== Download Requests ====================

    /// Save a download request
    pub async fn save_request(&self, request: &DownloadRequest) -> Result<()> {
        let options = UpdateOptions::builder().upsert(true).build();

        let update_doc = doc! {
            "$set": {
                "user_id": request.user_id,
                "username": &request.username,
                "content_id": &request.content_id,
                "content_type": &request.content_type,
                "title": &request.title,
                "series_title": &request.series_title,
                "status": format!("{:?}", request.status).to_lowercase(),
                "from_cache": request.from_cache,
                "error": &request.error,
                "requested_at": to_bson_datetime(request.requested_at),
                "completed_at": request.completed_at.map(to_bson_datetime),
            }
        };

        self.requests_collection()
            .update_one(doc! { "_id": &request.request_id }, update_doc)
            .with_options(options)
            .await
            .map_err(|e| Error::Database(format!("Failed to save request: {}", e)))?;

        Ok(())
    }

    /// Update request status
    pub async fn update_request_status(
        &self,
        request_id: &str,
        status: RequestStatus,
        error: Option<&str>,
    ) -> Result<()> {
        let mut update = doc! {
            "status": format!("{:?}", status).to_lowercase(),
        };

        if status == RequestStatus::Completed
            || status == RequestStatus::Failed
            || status == RequestStatus::Cached
        {
            update.insert("completed_at", BsonDateTime::now());
        }

        if let Some(err) = error {
            update.insert("error", err);
        }

        self.requests_collection()
            .update_one(doc! { "_id": request_id }, doc! { "$set": update })
            .await
            .map_err(|e| Error::Database(format!("Failed to update request status: {}", e)))?;

        Ok(())
    }

    /// Get recent requests by user
    pub async fn get_user_requests(&self, user_id: i64, limit: i64) -> Result<Vec<DownloadRequest>> {
        use futures::TryStreamExt;

        let cursor = self
            .requests_collection()
            .find(doc! { "user_id": user_id })
            .sort(doc! { "requested_at": -1 })
            .limit(limit)
            .await
            .map_err(|e| Error::Database(format!("Failed to get user requests: {}", e)))?;

        cursor
            .try_collect()
            .await
            .map_err(|e| Error::Database(format!("Failed to collect requests: {}", e)))
    }

    /// Check if content was requested recently by user
    pub async fn was_recently_requested(&self, user_id: i64, content_id: &str) -> Result<bool> {
        use chrono::{Duration, Utc};

        let one_hour_ago = Utc::now() - Duration::hours(1);

        let count = self
            .requests_collection()
            .count_documents(doc! {
                "user_id": user_id,
                "content_id": content_id,
                "requested_at": { "$gte": to_bson_datetime(one_hour_ago) }
            })
            .await
            .map_err(|e| Error::Database(format!("Failed to check recent requests: {}", e)))?;

        Ok(count > 0)
    }

    // ==================== Active Downloads ====================

    /// Get an active download by content ID
    pub async fn get_active_download(&self, content_id: &str) -> Result<Option<ActiveDownload>> {
        self.active_downloads_collection()
            .find_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to get active download: {}", e)))
    }

    /// Create a new active download (returns false if already exists)
    pub async fn create_active_download(&self, download: &ActiveDownload) -> Result<bool> {
        // Use insert to fail if already exists
        match self.active_downloads_collection()
            .insert_one(download)
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                // Check if it's a duplicate key error
                if e.to_string().contains("duplicate key") || e.to_string().contains("E11000") {
                    Ok(false)
                } else {
                    Err(Error::Database(format!("Failed to create active download: {}", e)))
                }
            }
        }
    }

    /// Update active download progress
    pub async fn update_active_download_progress(
        &self,
        content_id: &str,
        phase: &str,
        progress: u8,
        downloaded_bytes: u64,
        estimated_size: Option<u64>,
        speed: Option<u64>,
    ) -> Result<()> {
        let mut update = doc! {
            "phase": phase,
            "progress": progress as i32,
            "downloaded_bytes": downloaded_bytes as i64,
        };

        if let Some(size) = estimated_size {
            update.insert("estimated_size", size as i64);
        }
        if let Some(s) = speed {
            update.insert("speed", s as i64);
        }

        self.active_downloads_collection()
            .update_one(
                doc! { "_id": content_id },
                doc! { "$set": update },
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to update active download: {}", e)))?;

        Ok(())
    }

    /// Remove an active download (when completed or failed)
    pub async fn remove_active_download(&self, content_id: &str) -> Result<()> {
        self.active_downloads_collection()
            .delete_one(doc! { "_id": content_id })
            .await
            .map_err(|e| Error::Database(format!("Failed to remove active download: {}", e)))?;

        Ok(())
    }

    /// Clean up stale active downloads (older than 30 minutes - likely crashed)
    pub async fn cleanup_stale_active_downloads(&self) -> Result<u64> {
        use chrono::{Duration, Utc};

        let thirty_min_ago = Utc::now() - Duration::minutes(30);

        let result = self.active_downloads_collection()
            .delete_many(doc! { "started_at": { "$lt": to_bson_datetime(thirty_min_ago) } })
            .await
            .map_err(|e| Error::Database(format!("Failed to cleanup stale active downloads: {}", e)))?;

        Ok(result.deleted_count)
    }

    /// Get all active downloads (for startup cleanup)
    pub async fn get_all_active_downloads(&self) -> Result<Vec<ActiveDownload>> {
        use futures::TryStreamExt;

        let cursor = self.active_downloads_collection()
            .find(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to get all active downloads: {}", e)))?;

        let downloads: Vec<ActiveDownload> = cursor
            .try_collect()
            .await
            .map_err(|e| Error::Database(format!("Failed to collect active downloads: {}", e)))?;

        Ok(downloads)
    }

    /// Clear all active downloads (for startup cleanup)
    pub async fn clear_all_active_downloads(&self) -> Result<u64> {
        let result = self.active_downloads_collection()
            .delete_many(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to clear all active downloads: {}", e)))?;

        Ok(result.deleted_count)
    }

    // ==================== Statistics ====================

    /// Get bot statistics
    pub async fn get_stats(&self) -> Result<BotStats> {
        use futures::TryStreamExt;

        // Get active downloads with details
        let active_downloads_list = self.get_all_active_downloads().await?;
        let active_downloads_count = active_downloads_list.len() as u64;

        // Count authorized users (admins + authorized chats)
        let admin_count = self.admins_collection()
            .count_documents(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to count admins: {}", e)))?;

        let authorized_chat_count = self.authorized_chats_collection()
            .count_documents(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to count authorized chats: {}", e)))?;

        // Count total episodes decrypted (from cached_keys)
        let episodes_decrypted = self.keys_collection()
            .count_documents(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to count decrypted episodes: {}", e)))?;

        // Count cached files and calculate total size
        let telegram_files: Vec<CachedFile> = self.files_collection()
            .find(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to get telegram files: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| Error::Database(format!("Failed to collect telegram files: {}", e)))?;

        let telegram_count = telegram_files.len() as u64;
        let telegram_size: u64 = telegram_files.iter().map(|f| f.file_size).sum();
        let telegram_serve_count: u64 = telegram_files.iter().map(|f| f.forward_count as u64).sum();

        // Pixeldrain cache stats
        let pixeldrain_files: Vec<CachedPixeldrainFile> = self.pixeldrain_collection()
            .find(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to get pixeldrain files: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| Error::Database(format!("Failed to collect pixeldrain files: {}", e)))?;

        let pixeldrain_count = pixeldrain_files.len() as u64;
        let pixeldrain_size: u64 = pixeldrain_files.iter().map(|f| f.file_size).sum();
        let pixeldrain_serve_count: u64 = pixeldrain_files.iter().map(|f| f.serve_count as u64).sum();

        // Buzzheavier cache stats
        let buzzheavier_files: Vec<CachedBuzzheavierFile> = self.buzzheavier_collection()
            .find(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to get buzzheavier files: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| Error::Database(format!("Failed to collect buzzheavier files: {}", e)))?;

        let buzzheavier_count = buzzheavier_files.len() as u64;
        let buzzheavier_size: u64 = buzzheavier_files.iter().map(|f| f.file_size).sum();
        let buzzheavier_serve_count: u64 = buzzheavier_files.iter().map(|f| f.serve_count as u64).sum();

        // Gofile cache stats
        let gofile_files: Vec<CachedGofileFile> = self.gofile_collection()
            .find(doc! {})
            .await
            .map_err(|e| Error::Database(format!("Failed to get gofile files: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| Error::Database(format!("Failed to collect gofile files: {}", e)))?;

        let gofile_count = gofile_files.len() as u64;
        let gofile_size: u64 = gofile_files.iter().map(|f| f.file_size).sum();
        let gofile_serve_count: u64 = gofile_files.iter().map(|f| f.serve_count as u64).sum();

        // Total stats
        let total_cached_files = telegram_count + pixeldrain_count + buzzheavier_count + gofile_count;
        let total_cached_size = telegram_size + pixeldrain_size + buzzheavier_size + gofile_size;
        let total_serve_count = telegram_serve_count + pixeldrain_serve_count + buzzheavier_serve_count + gofile_serve_count;

        Ok(BotStats {
            active_downloads_count,
            active_downloads_list,
            admin_count,
            authorized_chat_count,
            episodes_decrypted,
            telegram_cache: CacheStats {
                file_count: telegram_count,
                total_size: telegram_size,
                serve_count: telegram_serve_count,
            },
            pixeldrain_cache: CacheStats {
                file_count: pixeldrain_count,
                total_size: pixeldrain_size,
                serve_count: pixeldrain_serve_count,
            },
            buzzheavier_cache: CacheStats {
                file_count: buzzheavier_count,
                total_size: buzzheavier_size,
                serve_count: buzzheavier_serve_count,
            },
            gofile_cache: CacheStats {
                file_count: gofile_count,
                total_size: gofile_size,
                serve_count: gofile_serve_count,
            },
            total_cached_files,
            total_cached_size,
            total_serve_count,
        })
    }
}

/// Statistics for a cache type
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub file_count: u64,
    pub total_size: u64,
    pub serve_count: u64,
}

/// Bot statistics
#[derive(Debug, Clone)]
pub struct BotStats {
    pub active_downloads_count: u64,
    pub active_downloads_list: Vec<ActiveDownload>,
    pub admin_count: u64,
    pub authorized_chat_count: u64,
    pub episodes_decrypted: u64,
    pub telegram_cache: CacheStats,
    pub pixeldrain_cache: CacheStats,
    pub buzzheavier_cache: CacheStats,
    pub gofile_cache: CacheStats,
    pub total_cached_files: u64,
    pub total_cached_size: u64,
    pub total_serve_count: u64,
}
