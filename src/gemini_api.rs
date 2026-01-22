use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
pub const MAX_INLINE_FILE_SIZE: u64 = 20 * 1024 * 1024; // 20MB limit for inline data
const DEFAULT_TIMEOUT_SECS: u64 = 600; // 10 minutes (large files need more time)
const DEFAULT_MAX_RETRIES: u32 = 3;

#[derive(Debug, Error)]
pub enum GeminiError {
    #[allow(dead_code)]
    #[error("API key not found in environment (set GEMINI_API_KEY or GOOGLE_AI_KEY)")]
    MissingApiKey,

    #[error("File too large: {size} bytes (max: {max} bytes). Consider using shorter audio.")]
    FileTooLarge { size: u64, max: u64 },

    #[error("Gemini API error ({status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("Rate limited by API. Retry after some time.")]
    RateLimited,

    #[error("Invalid response from Gemini API: {0}")]
    InvalidResponse(String),

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Max retries ({0}) exceeded")]
    MaxRetriesExceeded(u32),
}

pub type Result<T> = std::result::Result<T, GeminiError>;

#[derive(Debug, Clone)]
pub struct GeminiClientConfig {
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub model: String,
}

impl Default for GeminiClientConfig {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_retries: DEFAULT_MAX_RETRIES,
            model: "gemini-2.5-flash".to_string(),
        }
    }
}

/// Audio source for transcription - either inline data or a file URI
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AudioSource {
    /// Base64 inline data (for files <= 20MB)
    Inline { mime_type: String, data: Vec<u8> },
    /// File API URI (for files > 20MB)
    FileUri { mime_type: String, uri: String },
}

#[derive(Debug, Clone)]
pub struct GeminiClient {
    client: Client,
    api_key: String,
    config: GeminiClientConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub speaker: String,
    pub timestamp: String,
    pub content: String,
    pub language: String,
    pub language_code: String,
    #[serde(default)]
    pub translation: Option<String>,
    pub emotion: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptResponse {
    pub summary: String,
    pub segments: Vec<TranscriptSegment>,
}

impl GeminiClient {
    #[allow(dead_code)]
    pub fn new(api_key: String) -> Result<Self> {
        Self::with_config(api_key, GeminiClientConfig::default())
    }

    pub fn with_config(api_key: String, config: GeminiClientConfig) -> Result<Self> {
        let client = Client::builder()
            .use_rustls_tls()
            .timeout(Duration::from_secs(config.timeout_secs))
            .connect_timeout(Duration::from_secs(30))
            // Keep connections alive for long-running requests
            .pool_idle_timeout(Duration::from_secs(600))
            .pool_max_idle_per_host(2)
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .map_err(GeminiError::NetworkError)?;

        Ok(Self {
            client,
            api_key,
            config,
        })
    }

    #[allow(dead_code)]
    pub fn with_model(mut self, model: &str) -> Self {
        self.config.model = model.to_string();
        self
    }

    /// Get access to the underlying HTTP client for creating FileApiClient
    pub fn http_client(&self) -> &Client {
        &self.client
    }

    /// Get access to the API key for creating FileApiClient
    #[allow(dead_code)]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    fn encode_to_base64(data: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(data)
    }

    pub fn validate_file_size(size: u64) -> Result<()> {
        if size > MAX_INLINE_FILE_SIZE {
            return Err(GeminiError::FileTooLarge {
                size,
                max: MAX_INLINE_FILE_SIZE,
            });
        }
        Ok(())
    }

    /// Check if file size exceeds inline data limit
    #[allow(dead_code)]
    pub fn requires_file_api(size: u64) -> bool {
        size > MAX_INLINE_FILE_SIZE
    }

    pub fn get_mime_type(path: &Path) -> &'static str {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .as_deref()
        {
            Some("mp3") => "audio/mpeg",
            Some("wav") => "audio/wav",
            Some("ogg") => "audio/ogg",
            Some("flac") => "audio/flac",
            Some("m4a") => "audio/mp4",
            Some("aac") => "audio/aac",
            Some("wma") => "audio/x-ms-wma",
            Some("webm") => "audio/webm",
            _ => "audio/mpeg",
        }
    }

    async fn send_request(&self, payload: &Value) -> Result<TranscriptResponse> {
        let url = format!(
            "{}/{}:generateContent?key={}",
            GEMINI_API_URL, self.config.model, self.api_key
        );

        debug!("Sending request to Gemini API");

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(payload)
            .send()
            .await?;

        let status = response.status();
        debug!("Received response with status: {}", status);

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(GeminiError::RateLimited);
        }

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(GeminiError::ApiError {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let data: Value = response.json().await?;

        let text = data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| GeminiError::InvalidResponse("Missing text in response".to_string()))?;

        let transcript: TranscriptResponse = serde_json::from_str(text)?;
        Ok(transcript)
    }

    fn is_retryable_error(err: &GeminiError) -> bool {
        match err {
            GeminiError::RateLimited | GeminiError::NetworkError(_) => true,
            GeminiError::ApiError { status, .. } => *status >= 500,
            _ => false,
        }
    }

    fn get_transcription_prompt() -> &'static str {
        r#"Process the audio file and generate a detailed transcription.

Requirements:
1. Identify distinct speakers (e.g., Speaker 1, Speaker 2, or names if context allows).
2. Provide accurate timestamps for each segment (Format: MM:SS).
3. Detect the primary language of each segment.
4. If the segment is in a language different than English, also provide the English translation.
5. Identify the primary emotion of the speaker in this segment. You MUST choose exactly one of the following: Happy, Sad, Angry, Neutral.
6. Provide a brief summary of the entire audio at the beginning."#
    }

    fn get_generation_config() -> Value {
        json!({
            "response_mime_type": "application/json",
            "response_schema": {
                "type": "OBJECT",
                "properties": {
                    "summary": {
                        "type": "STRING",
                        "description": "A concise summary of the audio content."
                    },
                    "segments": {
                        "type": "ARRAY",
                        "description": "List of transcribed segments with speaker and timestamp.",
                        "items": {
                            "type": "OBJECT",
                            "properties": {
                                "speaker": { "type": "STRING" },
                                "timestamp": { "type": "STRING" },
                                "content": { "type": "STRING" },
                                "language": { "type": "STRING" },
                                "language_code": { "type": "STRING" },
                                "translation": { "type": "STRING" },
                                "emotion": {
                                    "type": "STRING",
                                    "enum": ["happy", "sad", "angry", "neutral"]
                                }
                            },
                            "required": ["speaker", "timestamp", "content", "language", "language_code", "emotion"]
                        }
                    }
                },
                "required": ["summary", "segments"]
            }
        })
    }

    async fn send_request_with_retry(&self, payload: &Value) -> Result<TranscriptResponse> {
        let mut last_error = None;
        let mut retry_count = 0;

        while retry_count < self.config.max_retries {
            match self.send_request(payload).await {
                Ok(response) => {
                    info!("Transcription successful");
                    return Ok(response);
                }
                Err(e) => {
                    if Self::is_retryable_error(&e) && retry_count + 1 < self.config.max_retries {
                        // Use longer backoff for rate limiting (30s base), shorter for other errors
                        let delay = if matches!(e, GeminiError::RateLimited) {
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

        Err(last_error.unwrap_or(GeminiError::MaxRetriesExceeded(self.config.max_retries)))
    }

    /// Transcribe audio using inline base64 data (for files <= 20MB)
    pub async fn transcribe_audio(
        &self,
        audio_data: &[u8],
        mime_type: &str,
    ) -> Result<TranscriptResponse> {
        Self::validate_file_size(audio_data.len() as u64)?;

        let base64_audio = Self::encode_to_base64(audio_data);
        let prompt = Self::get_transcription_prompt();

        let payload = json!({
            "contents": [
                {
                    "parts": [
                        {"text": prompt},
                        {
                            "inline_data": {
                                "mime_type": mime_type,
                                "data": base64_audio
                            }
                        }
                    ]
                }
            ],
            "generation_config": Self::get_generation_config()
        });

        self.send_request_with_retry(&payload).await
    }

    /// Transcribe audio using a file URI (for files uploaded via File API)
    pub async fn transcribe_file_uri(
        &self,
        file_uri: &str,
        mime_type: &str,
    ) -> Result<TranscriptResponse> {
        let prompt = Self::get_transcription_prompt();

        let payload = json!({
            "contents": [
                {
                    "parts": [
                        {"text": prompt},
                        {
                            "file_data": {
                                "mime_type": mime_type,
                                "file_uri": file_uri
                            }
                        }
                    ]
                }
            ],
            "generation_config": Self::get_generation_config()
        });

        self.send_request_with_retry(&payload).await
    }

    /// Transcribe audio from any source (inline data or file URI)
    #[allow(dead_code)]
    pub async fn transcribe_source(&self, source: &AudioSource) -> Result<TranscriptResponse> {
        match source {
            AudioSource::Inline { mime_type, data } => self.transcribe_audio(data, mime_type).await,
            AudioSource::FileUri { mime_type, uri } => {
                self.transcribe_file_uri(uri, mime_type).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_mime_type() {
        assert_eq!(
            GeminiClient::get_mime_type(Path::new("test.mp3")),
            "audio/mpeg"
        );
        assert_eq!(
            GeminiClient::get_mime_type(Path::new("test.MP3")),
            "audio/mpeg"
        );
        assert_eq!(
            GeminiClient::get_mime_type(Path::new("test.wav")),
            "audio/wav"
        );
        assert_eq!(
            GeminiClient::get_mime_type(Path::new("test.ogg")),
            "audio/ogg"
        );
        assert_eq!(
            GeminiClient::get_mime_type(Path::new("test.flac")),
            "audio/flac"
        );
        assert_eq!(
            GeminiClient::get_mime_type(Path::new("test.m4a")),
            "audio/mp4"
        );
        assert_eq!(
            GeminiClient::get_mime_type(Path::new("test.unknown")),
            "audio/mpeg"
        );
    }

    #[test]
    fn test_validate_file_size() {
        assert!(GeminiClient::validate_file_size(1024).is_ok());
        assert!(GeminiClient::validate_file_size(MAX_INLINE_FILE_SIZE).is_ok());
        assert!(GeminiClient::validate_file_size(MAX_INLINE_FILE_SIZE + 1).is_err());
    }

    #[test]
    fn test_encode_to_base64() {
        let data = b"hello world";
        let encoded = GeminiClient::encode_to_base64(data);
        assert_eq!(encoded, "aGVsbG8gd29ybGQ=");
    }
}
