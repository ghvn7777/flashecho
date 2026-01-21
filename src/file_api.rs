use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, info, warn};

const FILE_API_URL: &str = "https://generativelanguage.googleapis.com/upload/v1beta/files";
const FILE_INFO_URL: &str = "https://generativelanguage.googleapis.com/v1beta/files";
const FILE_PROCESSING_TIMEOUT_SECS: u64 = 300; // 5 minutes
const FILE_PROCESSING_POLL_INTERVAL_SECS: u64 = 2;

#[derive(Debug, Error)]
pub enum FileApiError {
    #[error("Failed to initiate upload: {0}")]
    UploadInitFailed(String),

    #[error("Missing upload URL in response headers")]
    MissingUploadUrl,

    #[error("Failed to upload file bytes: {0}")]
    UploadFailed(String),

    #[error("File processing timeout after {0} seconds")]
    FileProcessingTimeout(u64),

    #[error("Failed to delete file: {0}")]
    DeleteFailed(String),

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("API error ({status}): {message}")]
    ApiError { status: u16, message: String },
}

pub type Result<T> = std::result::Result<T, FileApiError>;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub name: String,
    pub uri: String,
    pub mime_type: String,
    pub size_bytes: String,
    pub state: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileResponse {
    file: FileInfo,
}

#[derive(Debug, Serialize)]
struct UploadMetadata {
    file: FileMetadata,
}

#[derive(Debug, Serialize)]
struct FileMetadata {
    display_name: String,
}

pub struct FileApiClient {
    client: Client,
    api_key: String,
}

impl FileApiClient {
    pub fn new(client: Client, api_key: String) -> Self {
        Self { client, api_key }
    }

    /// Step 1: Initiate resumable upload
    /// Returns the upload URL from response headers
    pub async fn start_upload(
        &self,
        file_size: u64,
        mime_type: &str,
        display_name: &str,
    ) -> Result<String> {
        let url = format!("{}?key={}", FILE_API_URL, self.api_key);

        let metadata = UploadMetadata {
            file: FileMetadata {
                display_name: display_name.to_string(),
            },
        };

        debug!(
            "Starting resumable upload for file: {} ({} bytes)",
            display_name, file_size
        );

        let response = self
            .client
            .post(&url)
            .header("X-Goog-Upload-Protocol", "resumable")
            .header("X-Goog-Upload-Command", "start")
            .header("X-Goog-Upload-Header-Content-Length", file_size.to_string())
            .header("X-Goog-Upload-Header-Content-Type", mime_type)
            .header("Content-Type", "application/json")
            .json(&metadata)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(FileApiError::UploadInitFailed(format!(
                "HTTP {}: {}",
                status, error_text
            )));
        }

        let upload_url = response
            .headers()
            .get("x-goog-upload-url")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or(FileApiError::MissingUploadUrl)?;

        debug!("Got upload URL: {}", upload_url);
        Ok(upload_url)
    }

    /// Step 2: Upload file bytes to the upload URL
    /// Returns FileInfo with the file URI
    pub async fn upload_bytes(&self, upload_url: &str, data: &[u8]) -> Result<FileInfo> {
        debug!("Uploading {} bytes to upload URL", data.len());

        let response = self
            .client
            .post(upload_url)
            .header("Content-Length", data.len().to_string())
            .header("X-Goog-Upload-Offset", "0")
            .header("X-Goog-Upload-Command", "upload, finalize")
            .body(data.to_vec())
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(FileApiError::UploadFailed(format!(
                "HTTP {}: {}",
                status, error_text
            )));
        }

        let file_response: FileResponse = response.json().await?;
        info!(
            "File uploaded successfully: {} ({})",
            file_response.file.name, file_response.file.uri
        );
        Ok(file_response.file)
    }

    /// Extract file ID from name (strips "files/" prefix if present)
    fn extract_file_id(file_name: &str) -> &str {
        file_name.strip_prefix("files/").unwrap_or(file_name)
    }

    /// Get file info by name
    pub async fn get_file_info(&self, file_name: &str) -> Result<FileInfo> {
        let file_id = Self::extract_file_id(file_name);
        let url = format!("{}/{}?key={}", FILE_INFO_URL, file_id, self.api_key);

        let response = self.client.get(&url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(FileApiError::ApiError {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let file_info: FileInfo = response.json().await?;
        Ok(file_info)
    }

    /// Wait for file to become ACTIVE (processing complete)
    pub async fn wait_for_file_active(&self, file_name: &str) -> Result<FileInfo> {
        let start = Instant::now();
        let timeout = Duration::from_secs(FILE_PROCESSING_TIMEOUT_SECS);

        loop {
            let info = self.get_file_info(file_name).await?;

            if info.state == "ACTIVE" {
                debug!("File {} is now ACTIVE", file_name);
                return Ok(info);
            }

            if info.state == "FAILED" {
                return Err(FileApiError::UploadFailed(format!(
                    "File processing failed for {}",
                    file_name
                )));
            }

            if start.elapsed() > timeout {
                return Err(FileApiError::FileProcessingTimeout(
                    FILE_PROCESSING_TIMEOUT_SECS,
                ));
            }

            debug!("File {} is in state {}, waiting...", file_name, info.state);
            tokio::time::sleep(Duration::from_secs(FILE_PROCESSING_POLL_INTERVAL_SECS)).await;
        }
    }

    /// Convenience method: upload file in one call
    pub async fn upload_file(
        &self,
        data: &[u8],
        mime_type: &str,
        display_name: &str,
    ) -> Result<FileInfo> {
        let upload_url = self
            .start_upload(data.len() as u64, mime_type, display_name)
            .await?;

        let file_info = self.upload_bytes(&upload_url, data).await?;

        // Wait for file to be processed if it's not already ACTIVE
        if file_info.state != "ACTIVE" {
            return self.wait_for_file_active(&file_info.name).await;
        }

        Ok(file_info)
    }

    /// Delete uploaded file after use
    pub async fn delete_file(&self, file_name: &str) -> Result<()> {
        let file_id = Self::extract_file_id(file_name);
        let url = format!("{}/{}?key={}", FILE_INFO_URL, file_id, self.api_key);

        debug!("Deleting file: {}", file_name);

        let response = self.client.delete(&url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!("Failed to delete file {}: {}", file_name, error_text);
            return Err(FileApiError::DeleteFailed(format!(
                "HTTP {}: {}",
                status, error_text
            )));
        }

        info!("File deleted: {}", file_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_info_deserialization() {
        let json = r#"{
            "name": "files/abc123",
            "displayName": "AUDIO",
            "mimeType": "audio/mpeg",
            "sizeBytes": "52428800",
            "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc123",
            "state": "ACTIVE"
        }"#;

        let file_info: FileInfo = serde_json::from_str(json).unwrap();
        assert_eq!(file_info.name, "files/abc123");
        assert_eq!(file_info.mime_type, "audio/mpeg");
        assert_eq!(file_info.state, "ACTIVE");
        assert_eq!(file_info.display_name, Some("AUDIO".to_string()));
    }

    #[test]
    fn test_extract_file_id() {
        assert_eq!(FileApiClient::extract_file_id("files/abc123"), "abc123");
        assert_eq!(FileApiClient::extract_file_id("abc123"), "abc123");
        assert_eq!(
            FileApiClient::extract_file_id("files/4knqiglwmyp7"),
            "4knqiglwmyp7"
        );
    }
}
