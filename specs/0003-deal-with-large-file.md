# Technical Plan: Large File Support via Gemini File API

## Overview

Currently, the transcript tool uses inline base64-encoded data to send audio to the Gemini API. This approach has a 20MB limit. For files larger than 20MB, we need to use the Gemini File API which supports resumable uploads up to 2GB.

## Current Limitation

```rust
const MAX_INLINE_FILE_SIZE: u64 = 20 * 1024 * 1024; // 20MB
```

Files exceeding this limit currently return an error:
```
File too large: {size} bytes (max: 20971520 bytes). Consider using shorter audio.
```

## Solution: Gemini File API

The Gemini File API provides a two-step process:
1. Upload file to Google's servers (resumable upload)
2. Reference the uploaded file by URI in generateContent request

### API Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `https://generativelanguage.googleapis.com/upload/v1beta/files` | POST | Initiate resumable upload |
| `{upload_url}` | POST | Upload file bytes |
| `https://generativelanguage.googleapis.com/v1beta/files/{name}` | DELETE | Delete uploaded file |

---

## Implementation Plan

### Phase 1: Add File API Client

Create new module `src/file_api.rs`:

```rust
use serde::{Deserialize, Serialize};

const FILE_API_URL: &str = "https://generativelanguage.googleapis.com/upload/v1beta/files";

#[derive(Debug, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub uri: String,
    pub mime_type: String,
    pub size_bytes: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct FileResponse {
    file: FileInfo,
}

pub struct FileApiClient {
    client: reqwest::Client,
    api_key: String,
}

impl FileApiClient {
    /// Step 1: Initiate resumable upload
    /// Returns the upload URL from response headers
    pub async fn start_upload(
        &self,
        file_size: u64,
        mime_type: &str,
        display_name: &str,
    ) -> Result<String>;

    /// Step 2: Upload file bytes to the upload URL
    /// Returns FileInfo with the file URI
    pub async fn upload_bytes(
        &self,
        upload_url: &str,
        data: &[u8],
    ) -> Result<FileInfo>;

    /// Convenience method: upload file in one call
    pub async fn upload_file(
        &self,
        data: &[u8],
        mime_type: &str,
        display_name: &str,
    ) -> Result<FileInfo>;

    /// Delete uploaded file after use
    pub async fn delete_file(&self, file_name: &str) -> Result<()>;
}
```

### Phase 2: Modify GeminiClient

Update `src/gemini_api.rs` to support both inline and file-based requests:

```rust
pub enum AudioSource {
    /// Base64 inline data (for files <= 20MB)
    Inline { mime_type: String, data: Vec<u8> },
    /// File API URI (for files > 20MB)
    FileUri { mime_type: String, uri: String },
}

impl GeminiClient {
    /// Transcribe using inline data (existing method, renamed)
    pub async fn transcribe_inline(
        &self,
        audio_data: &[u8],
        mime_type: &str,
    ) -> Result<TranscriptResponse>;

    /// Transcribe using file URI
    pub async fn transcribe_file_uri(
        &self,
        file_uri: &str,
        mime_type: &str,
    ) -> Result<TranscriptResponse>;

    /// Auto-select method based on file size
    pub async fn transcribe_audio(
        &self,
        audio_data: &[u8],
        mime_type: &str,
        file_api: Option<&FileApiClient>,
    ) -> Result<TranscriptResponse>;
}
```

### Phase 3: Update Request Payload

For file-based requests, the payload changes from:

```json
{
  "parts": [
    {"text": "..."},
    {"inline_data": {"mime_type": "audio/mpeg", "data": "base64..."}}
  ]
}
```

To:

```json
{
  "parts": [
    {"text": "..."},
    {"file_data": {"mime_type": "audio/mpeg", "file_uri": "https://..."}}
  ]
}
```

### Phase 4: Update CLI

Add new CLI options in `src/convert.rs`:

```rust
#[derive(Parser, Debug)]
struct Args {
    // ... existing args ...

    /// Force use of File API even for small files
    #[arg(long)]
    force_file_api: bool,

    /// Keep uploaded file on server (don't delete after transcription)
    #[arg(long)]
    keep_remote_file: bool,

    /// File size threshold for auto File API (in MB)
    #[arg(long, default_value = "20")]
    file_api_threshold_mb: u64,
}
```

### Phase 5: Implement Upload Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                        Start Transcription                       │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
                    ┌───────────────────────┐
                    │  Check file size      │
                    │  or --force-file-api  │
                    └───────────────────────┘
                                │
              ┌─────────────────┴─────────────────┐
              │                                   │
              ▼                                   ▼
    ┌─────────────────┐                 ┌─────────────────┐
    │  <= 20MB        │                 │  > 20MB or      │
    │  Use inline     │                 │  --force-file   │
    └─────────────────┘                 └─────────────────┘
              │                                   │
              │                                   ▼
              │                         ┌─────────────────┐
              │                         │ 1. Start upload │
              │                         │    (get URL)    │
              │                         └─────────────────┘
              │                                   │
              │                                   ▼
              │                         ┌─────────────────┐
              │                         │ 2. Upload bytes │
              │                         │    (get URI)    │
              │                         └─────────────────┘
              │                                   │
              ▼                                   ▼
    ┌─────────────────┐                 ┌─────────────────┐
    │  generateContent│                 │  generateContent│
    │  with inline    │                 │  with file_uri  │
    └─────────────────┘                 └─────────────────┘
              │                                   │
              └─────────────────┬─────────────────┘
                                │
                                ▼
                    ┌───────────────────────┐
                    │  Parse transcript     │
                    └───────────────────────┘
                                │
                                ▼
                    ┌───────────────────────┐
                    │  Delete remote file   │
                    │  (unless --keep)      │
                    └───────────────────────┘
                                │
                                ▼
                    ┌───────────────────────┐
                    │  Output result        │
                    └───────────────────────┘
```

---

## API Request Details

### Step 1: Initiate Resumable Upload

**Request:**
```http
POST https://generativelanguage.googleapis.com/upload/v1beta/files
x-goog-api-key: {API_KEY}
X-Goog-Upload-Protocol: resumable
X-Goog-Upload-Command: start
X-Goog-Upload-Header-Content-Length: {FILE_SIZE}
X-Goog-Upload-Header-Content-Type: {MIME_TYPE}
Content-Type: application/json

{"file": {"display_name": "{DISPLAY_NAME}"}}
```

**Response Headers:**
```
x-goog-upload-url: https://storage.googleapis.com/...
```

### Step 2: Upload File Bytes

**Request:**
```http
POST {upload_url}
Content-Length: {FILE_SIZE}
X-Goog-Upload-Offset: 0
X-Goog-Upload-Command: upload, finalize

{binary file data}
```

**Response:**
```json
{
  "file": {
    "name": "files/abc123",
    "displayName": "AUDIO",
    "mimeType": "audio/mpeg",
    "sizeBytes": "52428800",
    "createTime": "2024-01-01T00:00:00.000Z",
    "updateTime": "2024-01-01T00:00:00.000Z",
    "expirationTime": "2024-01-03T00:00:00.000Z",
    "sha256Hash": "...",
    "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc123",
    "state": "ACTIVE"
  }
}
```

### Step 3: Generate Content with File URI

**Request:**
```http
POST https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent
x-goog-api-key: {API_KEY}
Content-Type: application/json

{
  "contents": [{
    "parts": [
      {"text": "Transcribe this audio..."},
      {"file_data": {"mime_type": "audio/mpeg", "file_uri": "{FILE_URI}"}}
    ]
  }],
  "generation_config": {...}
}
```

### Step 4: Delete File (Cleanup)

**Request:**
```http
DELETE https://generativelanguage.googleapis.com/v1beta/files/{file_name}
x-goog-api-key: {API_KEY}
```

---

## Error Handling

| Error | Cause | Action |
|-------|-------|--------|
| 400 Bad Request | Invalid file format | Return user-friendly error |
| 403 Forbidden | Invalid API key | Return authentication error |
| 413 Payload Too Large | File > 2GB | Return file too large error |
| 429 Rate Limited | Too many requests | Retry with backoff |
| 5xx Server Error | Server issue | Retry with backoff |

### File State Handling

After upload, file may be in `PROCESSING` state. Need to poll until `ACTIVE`:

```rust
async fn wait_for_file_active(
    &self,
    file_name: &str,
    timeout: Duration,
) -> Result<FileInfo> {
    let start = Instant::now();
    loop {
        let info = self.get_file_info(file_name).await?;
        if info.state == "ACTIVE" {
            return Ok(info);
        }
        if start.elapsed() > timeout {
            return Err(GeminiError::FileProcessingTimeout);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
```

---

## New Dependencies

None required - using existing `reqwest` client.

---

## File Structure Changes

```
src/
├── convert.rs          # Updated CLI
├── gemini_api.rs       # Updated with file_uri support
└── file_api.rs         # NEW: File API client
```

---

## Testing Plan

1. **Unit Tests**
   - Test upload URL extraction from headers
   - Test FileInfo parsing
   - Test payload generation for file_uri

2. **Integration Tests**
   - Upload small file via File API
   - Upload large file (>20MB)
   - Verify cleanup deletes file

3. **Manual Tests**
   - Test with 25MB audio file
   - Test with 100MB audio file
   - Test `--force-file-api` flag
   - Test `--keep-remote-file` flag

---

## Migration Notes

- Existing behavior unchanged for files <= 20MB
- Large files now supported (up to 2GB)
- No breaking changes to CLI interface
- New optional flags for advanced users
