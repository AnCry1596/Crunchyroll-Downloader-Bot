use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;
use wreq::redirect::Policy;
use wreq_util::Emulation;

/// Status of a required tool
#[derive(Debug, Clone)]
pub struct ToolStatus {
    pub name: String,
    pub available: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
}

/// Manages external tools (mp4decrypt, FFmpeg, N_m3u8DL-RE)
pub struct ToolManager {
    tools_dir: PathBuf,
}

/// Create an HTTP client with redirect and browser emulation
fn create_http_client() -> std::result::Result<wreq::Client, wreq::Error> {
    wreq::Client::builder()
        .redirect(Policy::limited(10))
        .emulation(Emulation::Chrome143)
        .build()
}

impl ToolManager {
    pub fn new(tools_dir: PathBuf) -> Self {
        Self { tools_dir }
    }

    /// Check all required tools and return their status
    pub async fn check_tools(&self) -> Vec<ToolStatus> {
        let mut statuses = Vec::new();

        statuses.push(self.check_n_m3u8dl_re().await);
        statuses.push(self.check_mp4decrypt().await);
        statuses.push(self.check_ffmpeg().await);

        statuses
    }

    /// Check if N_m3u8DL-RE is available
    pub async fn check_n_m3u8dl_re(&self) -> ToolStatus {
        let name = "N_m3u8DL-RE".to_string();

        // Check in tools directory first
        let local_path = self.get_tool_path("N_m3u8DL-RE");
        if local_path.exists() {
            if let Ok(version) = self.get_n_m3u8dl_re_version(&local_path).await {
                return ToolStatus {
                    name,
                    available: true,
                    path: Some(local_path),
                    version: Some(version),
                };
            }
        }

        // Check in system PATH
        if let Ok(output) = Command::new("N_m3u8DL-RE").arg("--version").output().await {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let version = if !stdout.is_empty() { stdout } else { stderr };
            return ToolStatus {
                name,
                available: true,
                path: None,
                version: if version.is_empty() {
                    None
                } else {
                    Some(version)
                },
            };
        }

        ToolStatus {
            name,
            available: false,
            path: None,
            version: None,
        }
    }

    /// Check if mp4decrypt is available
    pub async fn check_mp4decrypt(&self) -> ToolStatus {
        let name = "mp4decrypt".to_string();

        // Check in tools directory first
        let local_path = self.get_tool_path("mp4decrypt");
        if local_path.exists() {
            if let Ok(version) = self.get_mp4decrypt_version(&local_path).await {
                return ToolStatus {
                    name,
                    available: true,
                    path: Some(local_path),
                    version: Some(version),
                };
            }
        }

        // Check in system PATH
        if let Ok(output) = Command::new("mp4decrypt").arg("--version").output().await {
            // mp4decrypt outputs version to stderr, not stdout
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let version = if !stdout.is_empty() { stdout } else { stderr };
            return ToolStatus {
                name,
                available: true,
                path: None, // System PATH
                version: if version.is_empty() {
                    None
                } else {
                    Some(version)
                },
            };
        }

        ToolStatus {
            name,
            available: false,
            path: None,
            version: None,
        }
    }

    /// Check if FFmpeg is available
    pub async fn check_ffmpeg(&self) -> ToolStatus {
        let name = "ffmpeg".to_string();

        // Check in tools directory first
        let local_path = self.get_tool_path("ffmpeg");
        if local_path.exists() {
            if let Ok(version) = self.get_ffmpeg_version(&local_path).await {
                return ToolStatus {
                    name,
                    available: true,
                    path: Some(local_path),
                    version: Some(version),
                };
            }
        }

        // Check in system PATH
        if let Ok(output) = Command::new("ffmpeg").arg("-version").output().await {
            // ffmpeg usually outputs to stdout, but check stderr as fallback
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let version_line = stdout.lines().next().unwrap_or("").to_string();
            let version = if !version_line.is_empty() {
                version_line
            } else {
                stderr.lines().next().unwrap_or("").to_string()
            };
            return ToolStatus {
                name,
                available: true,
                path: None,
                version: if version.is_empty() {
                    None
                } else {
                    Some(version)
                },
            };
        }

        ToolStatus {
            name,
            available: false,
            path: None,
            version: None,
        }
    }

    /// Download and install N_m3u8DL-RE
    pub async fn install_n_m3u8dl_re(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.tools_dir).await.map_err(|e| {
            Error::ExternalTool {
                tool: "N_m3u8DL-RE".to_string(),
                message: format!("Failed to create tools directory: {}", e),
            }
        })?;

        let (url, archive_name, exe_path_in_archive) = Self::get_n_m3u8dl_re_download_info();

        if url.is_empty() {
            return Err(Error::ExternalTool {
                tool: "N_m3u8DL-RE".to_string(),
                message: "No download available for this platform".to_string(),
            });
        }

        tracing::info!("Downloading N_m3u8DL-RE from: {}", url);

        let client = create_http_client().map_err(|e| Error::ExternalTool {
            tool: "N_m3u8DL-RE".to_string(),
            message: format!("Failed to create HTTP client: {}", e),
        })?;
        let response = client.get(url).send().await.map_err(|e| Error::ExternalTool {
            tool: "N_m3u8DL-RE".to_string(),
            message: format!("Failed to download: {}", e),
        })?;

        if !response.status().is_success() {
            return Err(Error::ExternalTool {
                tool: "N_m3u8DL-RE".to_string(),
                message: format!("Download failed with status: {}", response.status()),
            });
        }

        let bytes = response.bytes().await.map_err(|e| Error::ExternalTool {
            tool: "N_m3u8DL-RE".to_string(),
            message: format!("Failed to read download: {}", e),
        })?;

        let archive_path = self.tools_dir.join(archive_name);
        fs::write(&archive_path, &bytes).await.map_err(|e| Error::ExternalTool {
            tool: "N_m3u8DL-RE".to_string(),
            message: format!("Failed to save archive: {}", e),
        })?;

        // Extract the archive
        self.extract_archive(&archive_path, &self.tools_dir, exe_path_in_archive, "N_m3u8DL-RE")
            .await?;

        // Clean up archive
        let _ = fs::remove_file(&archive_path).await;

        let tool_path = self.get_tool_path("N_m3u8DL-RE");
        if tool_path.exists() {
            tracing::info!("N_m3u8DL-RE installed successfully at: {:?}", tool_path);
            Ok(tool_path)
        } else {
            Err(Error::ExternalTool {
                tool: "N_m3u8DL-RE".to_string(),
                message: "Installation failed: executable not found after extraction".to_string(),
            })
        }
    }

    /// Download and install mp4decrypt (Bento4)
    pub async fn install_mp4decrypt(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.tools_dir).await.map_err(|e| {
            Error::ExternalTool {
                tool: "mp4decrypt".to_string(),
                message: format!("Failed to create tools directory: {}", e),
            }
        })?;

        let (url, archive_name, exe_path_in_archive) = Self::get_bento4_download_info();

        tracing::info!("Downloading Bento4 from: {}", url);

        // Download the archive
        let client = create_http_client().map_err(|e| Error::ExternalTool {
            tool: "mp4decrypt".to_string(),
            message: format!("Failed to create HTTP client: {}", e),
        })?;
        let response = client.get(url).send().await.map_err(|e| Error::ExternalTool {
            tool: "mp4decrypt".to_string(),
            message: format!("Failed to download: {}", e),
        })?;

        if !response.status().is_success() {
            return Err(Error::ExternalTool {
                tool: "mp4decrypt".to_string(),
                message: format!("Download failed with status: {}", response.status()),
            });
        }

        let bytes = response.bytes().await.map_err(|e| Error::ExternalTool {
            tool: "mp4decrypt".to_string(),
            message: format!("Failed to read download: {}", e),
        })?;

        let archive_path = self.tools_dir.join(&archive_name);
        fs::write(&archive_path, &bytes).await.map_err(|e| Error::ExternalTool {
            tool: "mp4decrypt".to_string(),
            message: format!("Failed to save archive: {}", e),
        })?;

        // Extract the archive
        self.extract_archive(&archive_path, &self.tools_dir, &exe_path_in_archive, "mp4decrypt")
            .await?;

        // Clean up archive
        let _ = fs::remove_file(&archive_path).await;

        let tool_path = self.get_tool_path("mp4decrypt");
        if tool_path.exists() {
            tracing::info!("mp4decrypt installed successfully at: {:?}", tool_path);
            Ok(tool_path)
        } else {
            Err(Error::ExternalTool {
                tool: "mp4decrypt".to_string(),
                message: "Installation failed: executable not found after extraction".to_string(),
            })
        }
    }

    /// Download and install FFmpeg
    pub async fn install_ffmpeg(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.tools_dir).await.map_err(|e| {
            Error::ExternalTool {
                tool: "ffmpeg".to_string(),
                message: format!("Failed to create tools directory: {}", e),
            }
        })?;

        let (url, archive_name, exe_path_in_archive) = Self::get_ffmpeg_download_info();

        tracing::info!("Downloading FFmpeg from: {}", url);

        let client = create_http_client().map_err(|e| Error::ExternalTool {
            tool: "ffmpeg".to_string(),
            message: format!("Failed to create HTTP client: {}", e),
        })?;
        let response = client.get(url).send().await.map_err(|e| Error::ExternalTool {
            tool: "ffmpeg".to_string(),
            message: format!("Failed to download: {}", e),
        })?;

        if !response.status().is_success() {
            return Err(Error::ExternalTool {
                tool: "ffmpeg".to_string(),
                message: format!("Download failed with status: {}", response.status()),
            });
        }

        let bytes = response.bytes().await.map_err(|e| Error::ExternalTool {
            tool: "ffmpeg".to_string(),
            message: format!("Failed to read download: {}", e),
        })?;

        let archive_path = self.tools_dir.join(&archive_name);
        fs::write(&archive_path, &bytes).await.map_err(|e| Error::ExternalTool {
            tool: "ffmpeg".to_string(),
            message: format!("Failed to save archive: {}", e),
        })?;

        // Extract the archive
        self.extract_archive(&archive_path, &self.tools_dir, &exe_path_in_archive, "ffmpeg")
            .await?;

        // Clean up archive
        let _ = fs::remove_file(&archive_path).await;

        let tool_path = self.get_tool_path("ffmpeg");
        if tool_path.exists() {
            tracing::info!("FFmpeg installed successfully at: {:?}", tool_path);
            Ok(tool_path)
        } else {
            Err(Error::ExternalTool {
                tool: "ffmpeg".to_string(),
                message: "Installation failed: executable not found after extraction".to_string(),
            })
        }
    }

    /// Ensure all tools are available, downloading if necessary
    pub async fn ensure_tools(&self) -> Result<()> {
        let n_m3u8dl_re_status = self.check_n_m3u8dl_re().await;
        let mp4decrypt_status = self.check_mp4decrypt().await;
        let ffmpeg_status = self.check_ffmpeg().await;

        if !n_m3u8dl_re_status.available {
            tracing::info!("N_m3u8DL-RE not found, downloading...");
            self.install_n_m3u8dl_re().await?;
        } else {
            tracing::info!(
                "N_m3u8DL-RE available: {:?} ({})",
                n_m3u8dl_re_status.path,
                n_m3u8dl_re_status.version.unwrap_or_default()
            );
        }

        if !mp4decrypt_status.available {
            tracing::info!("mp4decrypt not found, downloading...");
            self.install_mp4decrypt().await?;
        } else {
            tracing::info!(
                "mp4decrypt available: {:?} ({})",
                mp4decrypt_status.path,
                mp4decrypt_status.version.unwrap_or_default()
            );
        }

        if !ffmpeg_status.available {
            tracing::info!("FFmpeg not found, downloading...");
            self.install_ffmpeg().await?;
        } else {
            tracing::info!(
                "FFmpeg available: {:?} ({})",
                ffmpeg_status.path,
                ffmpeg_status.version.unwrap_or_default()
            );
        }

        Ok(())
    }

    /// Get the path to a tool executable
    fn get_tool_path(&self, tool_name: &str) -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            self.tools_dir.join(format!("{}.exe", tool_name))
        }
        #[cfg(not(target_os = "windows"))]
        {
            self.tools_dir.join(tool_name)
        }
    }

    /// Get N_m3u8DL-RE version
    async fn get_n_m3u8dl_re_version(&self, path: &Path) -> Result<String> {
        let output = Command::new(path)
            .arg("--version")
            .output()
            .await
            .map_err(|e| Error::ExternalTool {
                tool: "N_m3u8DL-RE".to_string(),
                message: e.to_string(),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !stdout.is_empty() {
            Ok(stdout)
        } else {
            Ok(stderr)
        }
    }

    /// Get mp4decrypt version
    /// Note: Bento4's mp4decrypt outputs version info to stderr, not stdout
    async fn get_mp4decrypt_version(&self, path: &Path) -> Result<String> {
        let output = Command::new(path)
            .arg("--version")
            .output()
            .await
            .map_err(|e| Error::ExternalTool {
                tool: "mp4decrypt".to_string(),
                message: e.to_string(),
            })?;

        // mp4decrypt outputs to stderr, not stdout
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        // Prefer stdout if available, otherwise use stderr
        if !stdout.is_empty() {
            Ok(stdout)
        } else {
            Ok(stderr)
        }
    }

    /// Get FFmpeg version
    /// Note: ffmpeg usually outputs to stdout, but check stderr as fallback
    async fn get_ffmpeg_version(&self, path: &Path) -> Result<String> {
        let output = Command::new(path)
            .arg("-version")
            .output()
            .await
            .map_err(|e| Error::ExternalTool {
                tool: "ffmpeg".to_string(),
                message: e.to_string(),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Prefer stdout, fallback to stderr
        let version_line = stdout.lines().next().unwrap_or("").to_string();
        if !version_line.is_empty() {
            Ok(version_line)
        } else {
            Ok(stderr.lines().next().unwrap_or("").to_string())
        }
    }

    /// Get N_m3u8DL-RE download URL and paths for current platform
    /// Returns (url, archive_name, exe_path_in_archive)
    fn get_n_m3u8dl_re_download_info() -> (&'static str, &'static str, &'static str) {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            (
                "https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.5.1-beta/N_m3u8DL-RE_v0.5.1-beta_win-x64_20251029.zip",
                "N_m3u8DL-RE-win64.zip",
                "N_m3u8DL-RE.exe",
            )
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            (
                "https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.5.1-beta/N_m3u8DL-RE_v0.5.1-beta_linux-x64_20251029.tar.gz",
                "N_m3u8DL-RE-linux64.tar.gz",
                "N_m3u8DL-RE",
            )
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            (
                "https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.5.1-beta/N_m3u8DL-RE_v0.5.1-beta_osx-x64_20251029.tar.gz",
                "N_m3u8DL-RE-macos.tar.gz",
                "N_m3u8DL-RE",
            )
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            (
                "https://github.com/nilaoda/N_m3u8DL-RE/releases/download/v0.5.1-beta/N_m3u8DL-RE_v0.5.1-beta_osx-arm64_20251029.tar.gz",
                "N_m3u8DL-RE-macos-arm.tar.gz",
                "N_m3u8DL-RE",
            )
        }
        #[cfg(not(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64")
        )))]
        {
            ("", "", "")
        }
    }

    /// Get Bento4 download URL and paths for current platform
    fn get_bento4_download_info() -> (&'static str, &'static str, &'static str) {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            (
                "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.x86_64-microsoft-win32.zip",
                "bento4-win64.zip",
                "Bento4-SDK-1-6-0-641.x86_64-microsoft-win32/bin/mp4decrypt.exe",
            )
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            (
                "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.x86_64-unknown-linux.zip",
                "bento4-linux64.zip",
                "Bento4-SDK-1-6-0-641.x86_64-unknown-linux/bin/mp4decrypt",
            )
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            (
                "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.x86_64-apple-macosx.zip",
                "bento4-macos.zip",
                "Bento4-SDK-1-6-0-641.x86_64-apple-macosx/bin/mp4decrypt",
            )
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            (
                "https://www.bok.net/Bento4/binaries/Bento4-SDK-1-6-0-641.universal-apple-macosx.zip",
                "bento4-macos-arm.zip",
                "Bento4-SDK-1-6-0-641.universal-apple-macosx/bin/mp4decrypt",
            )
        }
        #[cfg(not(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64")
        )))]
        {
            ("", "", "")
        }
    }

    /// Get FFmpeg download URL and paths for current platform
    fn get_ffmpeg_download_info() -> (&'static str, &'static str, &'static str) {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            (
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
                "ffmpeg-win64.zip",
                "ffmpeg-master-latest-win64-gpl/bin/ffmpeg.exe",
            )
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            (
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz",
                "ffmpeg-linux64.tar.xz",
                "ffmpeg-master-latest-linux64-gpl/bin/ffmpeg",
            )
        }
        #[cfg(target_os = "macos")]
        {
            (
                "https://evermeet.cx/ffmpeg/getrelease/zip",
                "ffmpeg-macos.zip",
                "ffmpeg",
            )
        }
        #[cfg(not(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
            target_os = "macos"
        )))]
        {
            ("", "", "")
        }
    }

    /// Extract archive and copy the executable to tools directory
    async fn extract_archive(
        &self,
        archive_path: &Path,
        _dest_dir: &Path,
        exe_path_in_archive: &str,
        tool_name: &str,
    ) -> Result<()> {
        let archive_ext = archive_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match archive_ext {
            "zip" => self.extract_zip(archive_path, _dest_dir, exe_path_in_archive, tool_name).await,
            "xz" => self.extract_tar_xz(archive_path, _dest_dir, exe_path_in_archive, tool_name).await,
            "gz" => self.extract_tar_gz(archive_path, _dest_dir, exe_path_in_archive, tool_name).await,
            _ => Err(Error::ExternalTool {
                tool: tool_name.to_string(),
                message: format!("Unsupported archive format: {}", archive_ext),
            }),
        }
    }

    /// Extract a zip file
    async fn extract_zip(
        &self,
        archive_path: &Path,
        _dest_dir: &Path,
        exe_path_in_archive: &str,
        tool_name: &str,
    ) -> Result<()> {
        use std::io::Read;

        let file = std::fs::File::open(archive_path).map_err(|e| Error::ExternalTool {
            tool: tool_name.to_string(),
            message: format!("Failed to open archive: {}", e),
        })?;

        let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::ExternalTool {
            tool: tool_name.to_string(),
            message: format!("Failed to read zip archive: {}", e),
        })?;

        // Find and extract the executable
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| Error::ExternalTool {
                tool: tool_name.to_string(),
                message: format!("Failed to read archive entry: {}", e),
            })?;

            let file_path = file.name();
            if file_path == exe_path_in_archive || file_path.ends_with(&format!("/{}", tool_name))
                || file_path.ends_with(&format!("/{}.exe", tool_name))
            {
                let dest_path = self.get_tool_path(tool_name);
                let mut dest_file = std::fs::File::create(&dest_path).map_err(|e| Error::ExternalTool {
                    tool: tool_name.to_string(),
                    message: format!("Failed to create destination file: {}", e),
                })?;

                let mut contents = Vec::new();
                file.read_to_end(&mut contents).map_err(|e| Error::ExternalTool {
                    tool: tool_name.to_string(),
                    message: format!("Failed to read file from archive: {}", e),
                })?;

                std::io::Write::write_all(&mut dest_file, &contents).map_err(|e| Error::ExternalTool {
                    tool: tool_name.to_string(),
                    message: format!("Failed to write executable: {}", e),
                })?;

                // Make executable on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&dest_path)
                        .map_err(|e| Error::ExternalTool {
                            tool: tool_name.to_string(),
                            message: format!("Failed to get file permissions: {}", e),
                        })?
                        .permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&dest_path, perms).map_err(|e| Error::ExternalTool {
                        tool: tool_name.to_string(),
                        message: format!("Failed to set file permissions: {}", e),
                    })?;
                }

                return Ok(());
            }
        }

        Err(Error::ExternalTool {
            tool: tool_name.to_string(),
            message: format!("Executable not found in archive: {}", exe_path_in_archive),
        })
    }

    /// Extract a tar.xz file
    async fn extract_tar_xz(
        &self,
        archive_path: &Path,
        _dest_dir: &Path,
        exe_path_in_archive: &str,
        tool_name: &str,
    ) -> Result<()> {
        use std::io::Read;

        let file = std::fs::File::open(archive_path).map_err(|e| Error::ExternalTool {
            tool: tool_name.to_string(),
            message: format!("Failed to open archive: {}", e),
        })?;

        let decompressor = xz2::read::XzDecoder::new(file);
        let mut archive = tar::Archive::new(decompressor);

        for entry in archive.entries().map_err(|e| Error::ExternalTool {
            tool: tool_name.to_string(),
            message: format!("Failed to read tar archive: {}", e),
        })? {
            let mut entry = entry.map_err(|e| Error::ExternalTool {
                tool: tool_name.to_string(),
                message: format!("Failed to read archive entry: {}", e),
            })?;

            let path = entry.path().map_err(|e| Error::ExternalTool {
                tool: tool_name.to_string(),
                message: format!("Failed to get entry path: {}", e),
            })?;

            let path_str = path.to_string_lossy();
            if path_str == exe_path_in_archive
                || path_str.ends_with(&format!("/{}", tool_name))
            {
                let dest_path = self.get_tool_path(tool_name);
                let mut dest_file = std::fs::File::create(&dest_path).map_err(|e| Error::ExternalTool {
                    tool: tool_name.to_string(),
                    message: format!("Failed to create destination file: {}", e),
                })?;

                let mut contents = Vec::new();
                entry.read_to_end(&mut contents).map_err(|e| Error::ExternalTool {
                    tool: tool_name.to_string(),
                    message: format!("Failed to read file from archive: {}", e),
                })?;

                std::io::Write::write_all(&mut dest_file, &contents).map_err(|e| Error::ExternalTool {
                    tool: tool_name.to_string(),
                    message: format!("Failed to write executable: {}", e),
                })?;

                // Make executable on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&dest_path)
                        .map_err(|e| Error::ExternalTool {
                            tool: tool_name.to_string(),
                            message: format!("Failed to get file permissions: {}", e),
                        })?
                        .permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&dest_path, perms).map_err(|e| Error::ExternalTool {
                        tool: tool_name.to_string(),
                        message: format!("Failed to set file permissions: {}", e),
                    })?;
                }

                return Ok(());
            }
        }

        Err(Error::ExternalTool {
            tool: tool_name.to_string(),
            message: format!("Executable not found in archive: {}", exe_path_in_archive),
        })
    }

    /// Extract a tar.gz file
    async fn extract_tar_gz(
        &self,
        archive_path: &Path,
        _dest_dir: &Path,
        exe_path_in_archive: &str,
        tool_name: &str,
    ) -> Result<()> {
        use flate2::read::GzDecoder;
        use std::io::Read;

        let file = std::fs::File::open(archive_path).map_err(|e| Error::ExternalTool {
            tool: tool_name.to_string(),
            message: format!("Failed to open archive: {}", e),
        })?;

        let decompressor = GzDecoder::new(file);
        let mut archive = tar::Archive::new(decompressor);

        for entry in archive.entries().map_err(|e| Error::ExternalTool {
            tool: tool_name.to_string(),
            message: format!("Failed to read tar archive: {}", e),
        })? {
            let mut entry = entry.map_err(|e| Error::ExternalTool {
                tool: tool_name.to_string(),
                message: format!("Failed to read archive entry: {}", e),
            })?;

            let path = entry.path().map_err(|e| Error::ExternalTool {
                tool: tool_name.to_string(),
                message: format!("Failed to get entry path: {}", e),
            })?;

            let path_str = path.to_string_lossy();
            if path_str == exe_path_in_archive
                || path_str.ends_with(&format!("/{}", tool_name))
            {
                let dest_path = self.get_tool_path(tool_name);
                let mut dest_file = std::fs::File::create(&dest_path).map_err(|e| Error::ExternalTool {
                    tool: tool_name.to_string(),
                    message: format!("Failed to create destination file: {}", e),
                })?;

                let mut contents = Vec::new();
                entry.read_to_end(&mut contents).map_err(|e| Error::ExternalTool {
                    tool: tool_name.to_string(),
                    message: format!("Failed to read file from archive: {}", e),
                })?;

                std::io::Write::write_all(&mut dest_file, &contents).map_err(|e| Error::ExternalTool {
                    tool: tool_name.to_string(),
                    message: format!("Failed to write executable: {}", e),
                })?;

                // Make executable on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&dest_path)
                        .map_err(|e| Error::ExternalTool {
                            tool: tool_name.to_string(),
                            message: format!("Failed to get file permissions: {}", e),
                        })?
                        .permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&dest_path, perms).map_err(|e| Error::ExternalTool {
                        tool: tool_name.to_string(),
                        message: format!("Failed to set file permissions: {}", e),
                    })?;
                }

                return Ok(());
            }
        }

        Err(Error::ExternalTool {
            tool: tool_name.to_string(),
            message: format!("Executable not found in archive: {}", exe_path_in_archive),
        })
    }

    /// Get the command path for a tool (uses local if available, otherwise system PATH)
    pub async fn get_tool_command(&self, tool_name: &str) -> String {
        let local_path = self.get_tool_path(tool_name);
        if local_path.exists() {
            local_path.to_string_lossy().to_string()
        } else {
            tool_name.to_string()
        }
    }
}
