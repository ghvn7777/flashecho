# IDEA
Use rust base on current rust template implement a cli tool following process:
1. Use ffmpeg to extract audio from a video and save it as an mp3 file.
2. Use the gemini-2.5-flash API to convert the mp3 file into a transcript in JSON format.

GEMINI API Python example:
```python
api_key = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_AI_KEY")
if not api_key:
    raise ValueError("GEMINI_API_KEY (or GOOGLE_AI_KEY) environment variable is not set")

def encode_audio_to_base64(audio_path):
    with open(audio_path, "rb") as audio_file:
        return base64.b64encode(audio_file.read()).decode('utf-8')

# Gemini generateContent endpoint (matches the curl format in the comment above)
url = f"https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={api_key}"
headers = {
    "Content-Type": "application/json"
}

# Read and encode the audio file
audio_path = "/home/kaka/kaka/ai/openrouter/resourse/englishpod_C0075dg.mp3"
base64_audio = encode_audio_to_base64(audio_path)

payload = {
    "contents": [
        {
            "parts": [
                {"text": "Process the audio file and generate a detailed transcription.\n\nRequirements:\n1. Identify distinct speakers (e.g., Speaker 1, Speaker 2, or names if context allows).\n2. Provide accurate timestamps for each segment (Format: MM:SS).\n3. Detect the primary language of each segment.\n4. If the segment is in a language different than English, also provide the English translation.\n5. Identify the primary emotion of the speaker in this segment. You MUST choose exactly one of the following: Happy, Sad, Angry, Neutral.\n6. Provide a brief summary of the entire audio at the beginning."},
                {
                    "inline_data": {
                        # mp3 is typically audio/mpeg
                        "mime_type": "audio/mpeg",
                        "data": base64_audio,
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
}

response = requests.post(url, headers=headers, json=payload)
data = response.json()

text = data['candidates'][0]['content']['parts'][0]['text']

text_json = json.loads(text)
print(text_json)

# Write response JSON to a file named "<audio_path>.json"
out_path = f"{audio_path}.json"
with open(out_path, "w", encoding="utf-8") as f:
    json.dump(text_json, f, ensure_ascii=False, indent=2)
print(f"saved: {out_path}")
```

Use rust tokio, anyhow, reqwest(use rust-tls) crate

one gemini_api.rs for common GeminiAPI processing (for future use), one convert.rs as the CLI entrance and main function. No need to create other files.
