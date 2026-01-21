use anyhow::{Context, Result};
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Debug, Clone)]
pub struct GeminiClient {
    client: Client,
    api_key: String,
    model: String,
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
    pub fn new(api_key: String) -> Result<Self> {
        let client = Client::builder()
            .use_rustls_tls()
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            api_key,
            model: "gemini-2.5-flash".to_string(),
        })
    }

    #[allow(dead_code)]
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    fn encode_to_base64(data: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(data)
    }

    pub async fn transcribe_audio(
        &self,
        audio_data: &[u8],
        mime_type: &str,
    ) -> Result<TranscriptResponse> {
        let url = format!(
            "{}/{}:generateContent?key={}",
            GEMINI_API_URL, self.model, self.api_key
        );

        let base64_audio = Self::encode_to_base64(audio_data);

        let prompt = r#"Process the audio file and generate a detailed transcription.

Requirements:
1. Identify distinct speakers (e.g., Speaker 1, Speaker 2, or names if context allows).
2. Provide accurate timestamps for each segment (Format: MM:SS).
3. Detect the primary language of each segment.
4. If the segment is in a language different than English, also provide the English translation.
5. Identify the primary emotion of the speaker in this segment. You MUST choose exactly one of the following: Happy, Sad, Angry, Neutral.
6. Provide a brief summary of the entire audio at the beginning."#;

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
            "generation_config": {
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
            }
        });

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error ({}): {}", status, error_text);
        }

        let data: Value = response
            .json()
            .await
            .context("Failed to parse Gemini API response")?;

        let text = data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .context("Failed to extract text from Gemini response")?;

        let transcript: TranscriptResponse =
            serde_json::from_str(text).context("Failed to parse transcript JSON")?;

        Ok(transcript)
    }
}
