use base64::Engine;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MAX_RETRIES: u32 = 3;

/// Supported image generation models
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageModel {
    /// Gemini 2.5 Flash Image - fast generation
    #[default]
    Gemini25Flash,
    /// Gemini 3 Pro Image Preview - higher quality with size/aspect options
    Gemini3Pro,
}

impl ImageModel {
    pub fn api_model_name(&self) -> &'static str {
        match self {
            ImageModel::Gemini25Flash => "gemini-2.5-flash-image",
            ImageModel::Gemini3Pro => "gemini-3-pro-image-preview",
        }
    }

    /// Check if this model supports image configuration (size, aspect ratio)
    pub fn supports_image_config(&self) -> bool {
        matches!(self, ImageModel::Gemini3Pro)
    }
}

impl fmt::Display for ImageModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageModel::Gemini25Flash => write!(f, "2.5-flash"),
            ImageModel::Gemini3Pro => write!(f, "3pro"),
        }
    }
}

impl FromStr for ImageModel {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "2.5-flash" | "flash" | "gemini-2.5-flash-image" => Ok(ImageModel::Gemini25Flash),
            "3pro" | "3-pro" | "pro" | "gemini-3-pro-image-preview" => Ok(ImageModel::Gemini3Pro),
            _ => Err(format!("Unknown model: {}. Use '2.5-flash' or '3pro'", s)),
        }
    }
}

/// Image size options (Gemini 3 Pro only)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageSize {
    #[default]
    K1,
    K2,
    K4,
}

impl ImageSize {
    pub fn api_value(&self) -> &'static str {
        match self {
            ImageSize::K1 => "1K",
            ImageSize::K2 => "2K",
            ImageSize::K4 => "4K",
        }
    }
}

impl fmt::Display for ImageSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.api_value())
    }
}

impl FromStr for ImageSize {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        // Require exact case - uppercase K is mandatory per Gemini API spec
        match s {
            "1K" => Ok(ImageSize::K1),
            "2K" => Ok(ImageSize::K2),
            "4K" => Ok(ImageSize::K4),
            _ => Err(format!(
                "Invalid image size: {}. Must be 1K, 2K, or 4K (uppercase K required)",
                s
            )),
        }
    }
}

/// Aspect ratio options (Gemini 3 Pro only)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AspectRatio {
    #[default]
    Square, // 1:1
    Wide,     // 16:9
    Tall,     // 9:16
    Standard, // 4:3
    Portrait, // 3:4
}

impl AspectRatio {
    pub fn api_value(&self) -> &'static str {
        match self {
            AspectRatio::Square => "1:1",
            AspectRatio::Wide => "16:9",
            AspectRatio::Tall => "9:16",
            AspectRatio::Standard => "4:3",
            AspectRatio::Portrait => "3:4",
        }
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.api_value())
    }
}

impl FromStr for AspectRatio {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "1:1" | "square" => Ok(AspectRatio::Square),
            "16:9" | "wide" => Ok(AspectRatio::Wide),
            "9:16" | "tall" => Ok(AspectRatio::Tall),
            "4:3" | "standard" => Ok(AspectRatio::Standard),
            "3:4" | "portrait" => Ok(AspectRatio::Portrait),
            _ => Err(format!(
                "Invalid aspect ratio: {}. Use 1:1, 16:9, 9:16, 4:3, or 3:4",
                s
            )),
        }
    }
}

#[derive(Debug, Error)]
pub enum ImagenError {
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

    #[error("Image config (size/aspect) only supported with Gemini 3 Pro model")]
    ImageConfigNotSupported,
}

pub type Result<T> = std::result::Result<T, ImagenError>;

#[derive(Debug, Clone)]
pub struct ImagenClientConfig {
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub model: ImageModel,
}

impl Default for ImagenClientConfig {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_retries: DEFAULT_MAX_RETRIES,
            model: ImageModel::default(),
        }
    }
}

/// Image generation configuration for a request
#[derive(Debug, Clone, Default)]
pub struct ImageGenConfig {
    pub size: Option<ImageSize>,
    pub aspect_ratio: Option<AspectRatio>,
}

impl ImageGenConfig {
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

#[derive(Debug, Clone)]
pub struct ImagenClient {
    client: Client,
    api_key: String,
    config: ImagenClientConfig,
}

/// Response part from Gemini API - can be text or image
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ResponsePart {
    Image { inline_data: InlineData },
    Text { text: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct InlineData {
    pub mime_type: String,
    pub data: String,
}

/// Generated image data
#[derive(Debug, Clone)]
pub struct GeneratedImage {
    pub data: Vec<u8>,
    pub mime_type: String,
}

impl GeneratedImage {
    /// Get the appropriate file extension for this image
    pub fn extension(&self) -> &'static str {
        match self.mime_type.as_str() {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            "image/webp" => "webp",
            "image/gif" => "gif",
            _ => "png",
        }
    }
}

impl ImagenClient {
    pub fn new(api_key: String) -> Result<Self> {
        Self::with_config(api_key, ImagenClientConfig::default())
    }

    pub fn with_config(api_key: String, config: ImagenClientConfig) -> Result<Self> {
        let client = Client::builder()
            .use_rustls_tls()
            .timeout(Duration::from_secs(config.timeout_secs))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(ImagenError::NetworkError)?;

        Ok(Self {
            client,
            api_key,
            config,
        })
    }

    fn build_payload(&self, prompt: &str, gen_config: Option<&ImageGenConfig>) -> Value {
        match self.config.model {
            ImageModel::Gemini25Flash => {
                // Simple payload for Gemini 2.5 Flash
                json!({
                    "contents": [{
                        "parts": [{"text": prompt}]
                    }]
                })
            }
            ImageModel::Gemini3Pro => {
                // Gemini 3 Pro with image config support
                let mut image_config = json!({});

                if let Some(cfg) = gen_config {
                    if let Some(ratio) = &cfg.aspect_ratio {
                        image_config["aspectRatio"] = json!(ratio.api_value());
                    }
                    if let Some(size) = &cfg.size {
                        image_config["imageSize"] = json!(size.api_value());
                    }
                }

                // Default aspect ratio if not specified
                if image_config.get("aspectRatio").is_none() {
                    image_config["aspectRatio"] = json!("1:1");
                }
                // Default size if not specified
                if image_config.get("imageSize").is_none() {
                    image_config["imageSize"] = json!("1K");
                }

                json!({
                    "contents": [{"parts": [{"text": prompt}]}],
                    "generationConfig": {
                        "responseModalities": ["TEXT", "IMAGE"],
                        "imageConfig": image_config
                    }
                })
            }
        }
    }

    async fn send_request(
        &self,
        prompt: &str,
        gen_config: Option<&ImageGenConfig>,
    ) -> Result<GeneratedImage> {
        let url = format!(
            "{}/{}:generateContent?key={}",
            GEMINI_API_URL,
            self.config.model.api_model_name(),
            self.api_key
        );

        let payload = self.build_payload(prompt, gen_config);

        debug!(
            "Sending image generation request to Gemini API (model: {})",
            self.config.model
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
            return Err(ImagenError::RateLimited);
        }

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ImagenError::ApiError {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let data: Value = response.json().await?;

        // Extract image from response
        // Response structure: candidates[0].content.parts[] where parts can have inline_data
        let parts = data["candidates"][0]["content"]["parts"]
            .as_array()
            .ok_or_else(|| ImagenError::InvalidResponse("Missing parts in response".to_string()))?;

        for part in parts {
            if let Some(inline_data) = part.get("inlineData") {
                let mime_type = inline_data["mimeType"]
                    .as_str()
                    .unwrap_or("image/png")
                    .to_string();
                let base64_data = inline_data["data"].as_str().ok_or_else(|| {
                    ImagenError::InvalidResponse("Missing image data".to_string())
                })?;

                let image_data = base64::engine::general_purpose::STANDARD.decode(base64_data)?;

                return Ok(GeneratedImage {
                    data: image_data,
                    mime_type,
                });
            }
        }

        Err(ImagenError::NoImageData)
    }

    fn is_retryable_error(err: &ImagenError) -> bool {
        match err {
            ImagenError::RateLimited | ImagenError::NetworkError(_) => true,
            ImagenError::ApiError { status, .. } => *status >= 500,
            _ => false,
        }
    }

    /// Generate an image from a text prompt with retry logic
    pub async fn generate_image(&self, prompt: &str) -> Result<GeneratedImage> {
        self.generate_image_with_config(prompt, None).await
    }

    /// Generate an image with specific configuration (size, aspect ratio)
    pub async fn generate_image_with_config(
        &self,
        prompt: &str,
        gen_config: Option<&ImageGenConfig>,
    ) -> Result<GeneratedImage> {
        // Validate that image config is only used with Gemini 3 Pro
        if let Some(cfg) = gen_config
            && (cfg.size.is_some() || cfg.aspect_ratio.is_some())
            && !self.config.model.supports_image_config()
        {
            return Err(ImagenError::ImageConfigNotSupported);
        }

        let mut last_error = None;
        let mut retry_count = 0;

        while retry_count < self.config.max_retries {
            match self.send_request(prompt, gen_config).await {
                Ok(image) => {
                    info!("Image generation successful");
                    return Ok(image);
                }
                Err(e) => {
                    if Self::is_retryable_error(&e) && retry_count + 1 < self.config.max_retries {
                        let delay = if matches!(e, ImagenError::RateLimited) {
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

        Err(last_error.unwrap_or(ImagenError::MaxRetriesExceeded(self.config.max_retries)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generated_image_extension() {
        let png_image = GeneratedImage {
            data: vec![],
            mime_type: "image/png".to_string(),
        };
        assert_eq!(png_image.extension(), "png");

        let jpg_image = GeneratedImage {
            data: vec![],
            mime_type: "image/jpeg".to_string(),
        };
        assert_eq!(jpg_image.extension(), "jpg");

        let webp_image = GeneratedImage {
            data: vec![],
            mime_type: "image/webp".to_string(),
        };
        assert_eq!(webp_image.extension(), "webp");

        let unknown_image = GeneratedImage {
            data: vec![],
            mime_type: "image/unknown".to_string(),
        };
        assert_eq!(unknown_image.extension(), "png");
    }

    #[test]
    fn test_default_config() {
        let config = ImagenClientConfig::default();
        assert_eq!(config.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(config.max_retries, DEFAULT_MAX_RETRIES);
        assert_eq!(config.model, ImageModel::Gemini25Flash);
    }

    #[test]
    fn test_image_model_from_str() {
        assert_eq!(
            ImageModel::from_str("2.5-flash").unwrap(),
            ImageModel::Gemini25Flash
        );
        assert_eq!(
            ImageModel::from_str("flash").unwrap(),
            ImageModel::Gemini25Flash
        );
        assert_eq!(
            ImageModel::from_str("3pro").unwrap(),
            ImageModel::Gemini3Pro
        );
        assert_eq!(
            ImageModel::from_str("3-pro").unwrap(),
            ImageModel::Gemini3Pro
        );
        assert_eq!(ImageModel::from_str("pro").unwrap(), ImageModel::Gemini3Pro);
        assert!(ImageModel::from_str("invalid").is_err());
    }

    #[test]
    fn test_image_size_from_str() {
        assert_eq!(ImageSize::from_str("1K").unwrap(), ImageSize::K1);
        assert_eq!(ImageSize::from_str("2K").unwrap(), ImageSize::K2);
        assert_eq!(ImageSize::from_str("4K").unwrap(), ImageSize::K4);
        assert!(ImageSize::from_str("1k").is_err()); // lowercase not allowed
        assert!(ImageSize::from_str("8K").is_err());
    }

    #[test]
    fn test_aspect_ratio_from_str() {
        assert_eq!(AspectRatio::from_str("1:1").unwrap(), AspectRatio::Square);
        assert_eq!(
            AspectRatio::from_str("square").unwrap(),
            AspectRatio::Square
        );
        assert_eq!(AspectRatio::from_str("16:9").unwrap(), AspectRatio::Wide);
        assert_eq!(AspectRatio::from_str("wide").unwrap(), AspectRatio::Wide);
        assert_eq!(AspectRatio::from_str("9:16").unwrap(), AspectRatio::Tall);
        assert_eq!(AspectRatio::from_str("4:3").unwrap(), AspectRatio::Standard);
        assert_eq!(AspectRatio::from_str("3:4").unwrap(), AspectRatio::Portrait);
        assert!(AspectRatio::from_str("2:1").is_err());
    }

    #[test]
    fn test_model_api_names() {
        assert_eq!(
            ImageModel::Gemini25Flash.api_model_name(),
            "gemini-2.5-flash-image"
        );
        assert_eq!(
            ImageModel::Gemini3Pro.api_model_name(),
            "gemini-3-pro-image-preview"
        );
    }

    #[test]
    fn test_model_supports_image_config() {
        assert!(!ImageModel::Gemini25Flash.supports_image_config());
        assert!(ImageModel::Gemini3Pro.supports_image_config());
    }
}
