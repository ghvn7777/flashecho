use base64::Engine;
use reqwest::Client;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use tokio::fs;
use tracing::{debug, info, warn};

use crate::imagen_api::{AspectRatio, GeneratedImage, ImageSize};

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MAX_RETRIES: u32 = 3;
const MODEL_NAME: &str = "gemini-3-pro-image-preview";

#[derive(Debug, Error)]
pub enum ImageEditError {
    #[error("API key not found in environment (set GEMINI_API_KEY or GOOGLE_AI_KEY)")]
    MissingApiKey,

    #[error("Gemini API error ({status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("Rate limited by API. Retry after some time.")]
    RateLimited,

    #[error("Invalid response from Gemini API: {0}")]
    InvalidResponse(String),

    #[error("No image data in response")]
    NoImageData,

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Max retries ({0}) exceeded")]
    MaxRetriesExceeded(u32),

    #[error("Base64 decode error: {0}")]
    Base64Error(#[from] base64::DecodeError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("No input images provided")]
    NoInputImages,

    #[error("Unsupported image format: {0}")]
    UnsupportedFormat(String),
}

pub type Result<T> = std::result::Result<T, ImageEditError>;

/// Configuration for image editing client
#[derive(Debug, Clone)]
pub struct ImageEditClientConfig {
    pub timeout_secs: u64,
    pub max_retries: u32,
}

impl Default for ImageEditClientConfig {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

/// Configuration for image edit generation
#[derive(Debug, Clone, Default)]
pub struct ImageEditConfig {
    pub size: Option<ImageSize>,
    pub aspect_ratio: Option<AspectRatio>,
}

impl ImageEditConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_size(mut self, size: ImageSize) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_aspect_ratio(mut self, ratio: AspectRatio) -> Self {
        self.aspect_ratio = Some(ratio);
        self
    }
}

/// Input image data for editing
#[derive(Debug, Clone)]
pub struct InputImage {
    pub mime_type: String,
    pub data: Vec<u8>,
}

impl InputImage {
    /// Create from file path
    pub async fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let data = fs::read(path).await?;
        let mime_type = mime_type_from_path(path)?;
        Ok(Self { mime_type, data })
    }

    /// Create from raw bytes with explicit mime type
    pub fn from_bytes(data: Vec<u8>, mime_type: String) -> Self {
        Self { mime_type, data }
    }

    /// Get base64 encoded data
    pub fn base64_data(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }
}

/// Determine MIME type from file extension
fn mime_type_from_path(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "png" => Ok("image/png".to_string()),
        "jpg" | "jpeg" => Ok("image/jpeg".to_string()),
        "webp" => Ok("image/webp".to_string()),
        "gif" => Ok("image/gif".to_string()),
        "heic" => Ok("image/heic".to_string()),
        "heif" => Ok("image/heif".to_string()),
        _ => Err(ImageEditError::UnsupportedFormat(ext)),
    }
}

/// Client for image editing via Gemini API
#[derive(Debug, Clone)]
pub struct ImageEditClient {
    client: Client,
    api_key: String,
    config: ImageEditClientConfig,
}

impl ImageEditClient {
    pub fn new(api_key: String) -> Result<Self> {
        Self::with_config(api_key, ImageEditClientConfig::default())
    }

    pub fn with_config(api_key: String, config: ImageEditClientConfig) -> Result<Self> {
        let client = Client::builder()
            .use_rustls_tls()
            .timeout(Duration::from_secs(config.timeout_secs))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(ImageEditError::NetworkError)?;

        Ok(Self {
            client,
            api_key,
            config,
        })
    }

    fn build_payload(
        &self,
        prompt: &str,
        images: &[InputImage],
        edit_config: Option<&ImageEditConfig>,
    ) -> Value {
        // Build parts array: text prompt followed by all images
        let mut parts = vec![json!({"text": prompt})];

        for image in images {
            parts.push(json!({
                "inline_data": {
                    "mime_type": image.mime_type,
                    "data": image.base64_data()
                }
            }));
        }

        // Build image config
        let mut image_config = json!({});
        if let Some(cfg) = edit_config {
            if let Some(ratio) = &cfg.aspect_ratio {
                image_config["aspectRatio"] = json!(ratio.api_value());
            }
            if let Some(size) = &cfg.size {
                image_config["imageSize"] = json!(size.api_value());
            }
        }

        // Set defaults if not specified
        if image_config.get("aspectRatio").is_none() {
            image_config["aspectRatio"] = json!("1:1");
        }
        if image_config.get("imageSize").is_none() {
            image_config["imageSize"] = json!("1K");
        }

        json!({
            "contents": [{
                "parts": parts
            }],
            "generationConfig": {
                "responseModalities": ["TEXT", "IMAGE"],
                "imageConfig": image_config
            }
        })
    }

    async fn send_request(
        &self,
        prompt: &str,
        images: &[InputImage],
        edit_config: Option<&ImageEditConfig>,
    ) -> Result<GeneratedImage> {
        let url = format!(
            "{}/{}:generateContent?key={}",
            GEMINI_API_URL, MODEL_NAME, self.api_key
        );

        let payload = self.build_payload(prompt, images, edit_config);

        debug!(
            "Sending image edit request to Gemini API with {} input images",
            images.len()
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let status = response.status();
        debug!("Received response with status: {}", status);

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ImageEditError::RateLimited);
        }

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ImageEditError::ApiError {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let data: Value = response.json().await?;

        // Extract image from response
        let parts = data["candidates"][0]["content"]["parts"]
            .as_array()
            .ok_or_else(|| {
                ImageEditError::InvalidResponse("Missing parts in response".to_string())
            })?;

        for part in parts {
            if let Some(inline_data) = part.get("inlineData") {
                let mime_type = inline_data["mimeType"]
                    .as_str()
                    .unwrap_or("image/png")
                    .to_string();
                let base64_data = inline_data["data"].as_str().ok_or_else(|| {
                    ImageEditError::InvalidResponse("Missing image data".to_string())
                })?;

                let image_data = base64::engine::general_purpose::STANDARD.decode(base64_data)?;

                return Ok(GeneratedImage {
                    data: image_data,
                    mime_type,
                });
            }
        }

        Err(ImageEditError::NoImageData)
    }

    fn is_retryable_error(err: &ImageEditError) -> bool {
        match err {
            ImageEditError::RateLimited | ImageEditError::NetworkError(_) => true,
            ImageEditError::ApiError { status, .. } => *status >= 500,
            _ => false,
        }
    }

    /// Edit images with a text prompt
    pub async fn edit_images(&self, prompt: &str, images: &[InputImage]) -> Result<GeneratedImage> {
        self.edit_images_with_config(prompt, images, None).await
    }

    /// Edit images with a text prompt and specific configuration
    pub async fn edit_images_with_config(
        &self,
        prompt: &str,
        images: &[InputImage],
        edit_config: Option<&ImageEditConfig>,
    ) -> Result<GeneratedImage> {
        if images.is_empty() {
            return Err(ImageEditError::NoInputImages);
        }

        let mut last_error = None;
        let mut retry_count = 0;

        while retry_count < self.config.max_retries {
            match self.send_request(prompt, images, edit_config).await {
                Ok(image) => {
                    info!("Image edit successful");
                    return Ok(image);
                }
                Err(e) => {
                    if Self::is_retryable_error(&e) && retry_count + 1 < self.config.max_retries {
                        let delay = if matches!(e, ImageEditError::RateLimited) {
                            Duration::from_secs(30 * (retry_count as u64 + 1))
                        } else {
                            Duration::from_secs(2u64.pow(retry_count))
                        };
                        warn!(
                            "Request failed (attempt {}/{}): {}. Retrying in {:?}...",
                            retry_count + 1,
                            self.config.max_retries,
                            e,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        retry_count += 1;
                        last_error = Some(e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(last_error.unwrap_or(ImageEditError::MaxRetriesExceeded(self.config.max_retries)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mime_type_from_path() {
        assert_eq!(
            mime_type_from_path(Path::new("test.png")).unwrap(),
            "image/png"
        );
        assert_eq!(
            mime_type_from_path(Path::new("test.jpg")).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            mime_type_from_path(Path::new("test.jpeg")).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            mime_type_from_path(Path::new("test.webp")).unwrap(),
            "image/webp"
        );
        assert_eq!(
            mime_type_from_path(Path::new("test.gif")).unwrap(),
            "image/gif"
        );
        assert!(mime_type_from_path(Path::new("test.txt")).is_err());
    }

    #[test]
    fn test_default_config() {
        let config = ImageEditClientConfig::default();
        assert_eq!(config.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(config.max_retries, DEFAULT_MAX_RETRIES);
    }

    #[test]
    fn test_input_image_from_bytes() {
        let data = vec![1, 2, 3, 4];
        let image = InputImage::from_bytes(data.clone(), "image/png".to_string());
        assert_eq!(image.data, data);
        assert_eq!(image.mime_type, "image/png");
    }

    #[test]
    fn test_input_image_base64() {
        let data = vec![1, 2, 3, 4];
        let image = InputImage::from_bytes(data, "image/png".to_string());
        let base64 = image.base64_data();
        assert_eq!(base64, "AQIDBA==");
    }

    #[test]
    fn test_edit_config_builder() {
        let config = ImageEditConfig::new()
            .with_size(ImageSize::K2)
            .with_aspect_ratio(AspectRatio::Wide);
        assert_eq!(config.size, Some(ImageSize::K2));
        assert_eq!(config.aspect_ratio, Some(AspectRatio::Wide));
    }
}
