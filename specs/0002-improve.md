# Improvement Proposals for Transcript Tool

## Current State Analysis

The current implementation is functional but has several areas that could be improved for production readiness, better user experience, and robustness.

---

## Priority 1: Critical Improvements

### 1.1 Retry Logic & Rate Limiting

**Problem**: Network failures or API rate limits cause immediate failure with no recovery.

**Solution**:
```rust
// Add exponential backoff retry for transient failures
// Detect 429 (rate limit) and 5xx errors for retry
// Configure max retries and backoff parameters
```

**Implementation**:
- Add `reqwest-retry` or implement custom retry middleware
- Configurable retry count (default: 3)
- Exponential backoff: 1s, 2s, 4s...
- Only retry on transient errors (429, 500, 502, 503, 504)

### 1.2 File Size Validation

**Problem**: Gemini API has file size limits. Large files fail after upload.

**Solution**:
- Check file size before processing
- Warn user if file exceeds recommended size (~20MB for inline data)
- Suggest using File API for larger files

### 1.3 MIME Type Detection

**Problem**: Currently hardcoded to `audio/mpeg`, incorrect for other formats.

**Solution**:
```rust
fn get_mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("m4a") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("wma") => "audio/x-ms-wma",
        _ => "audio/mpeg", // default fallback
    }
}
```

### 1.4 API Timeout Configuration

**Problem**: No timeout on API requests; could hang indefinitely.

**Solution**:
```rust
let client = Client::builder()
    .use_rustls_tls()
    .timeout(Duration::from_secs(300)) // 5 min default
    .connect_timeout(Duration::from_secs(30))
    .build()?;
```

---

## Priority 2: User Experience Improvements

### 2.1 Progress Indication

**Problem**: No feedback during long operations.

**Solution**:
- Add `indicatif` crate for progress bars
- Show spinner during API calls
- Show progress during file read/write

```rust
// Example usage
let pb = ProgressBar::new_spinner();
pb.set_message("Sending audio to Gemini API...");
pb.enable_steady_tick(Duration::from_millis(100));
// ... API call ...
pb.finish_with_message("Transcription complete!");
```

### 2.2 Proper Logging

**Problem**: Using `println!` without verbosity control.

**Solution**:
- Add `tracing` or `env_logger` crate
- Support `-v`, `-vv`, `-vvv` verbosity levels
- Log to stderr, output to stdout

```rust
#[arg(short, long, action = clap::ArgAction::Count)]
verbose: u8,
```

### 2.3 Multiple Output Formats

**Problem**: Only JSON output supported.

**Solution**:
Add `--format` option supporting:
- `json` (default) - current structured format
- `srt` - SubRip subtitle format
- `vtt` - WebVTT subtitle format
- `txt` - Plain text transcript

```rust
#[arg(short, long, default_value = "json")]
format: OutputFormat,
```

### 2.4 Async ffmpeg Execution

**Problem**: ffmpeg blocks the async runtime.

**Solution**:
```rust
use tokio::process::Command;

async fn extract_audio_with_ffmpeg(input: &Path, output: &Path) -> Result<()> {
    let output_result = Command::new("ffmpeg")
        .args([...])
        .output()
        .await
        .context("Failed to execute ffmpeg")?;

    if !output_result.status.success() {
        let stderr = String::from_utf8_lossy(&output_result.stderr);
        anyhow::bail!("ffmpeg failed: {}", stderr);
    }
    Ok(())
}
```

---

## Priority 3: Feature Enhancements

### 3.1 Batch Processing

**Problem**: Can only process one file at a time.

**Solution**:
```rust
#[arg(short, long, num_args = 1..)]
input: Vec<PathBuf>,

// Or directory mode
#[arg(short = 'd', long)]
input_dir: Option<PathBuf>,

#[arg(long, default_value = "*.mp4,*.mkv,*.mp3")]
pattern: String,
```

Process files concurrently with configurable parallelism:
```rust
#[arg(long, default_value = "2")]
parallel: usize,
```

### 3.2 Configurable Transcription Options

**Problem**: Hardcoded prompt and model.

**Solution**:
```rust
#[arg(long, default_value = "gemini-2.5-flash")]
model: String,

#[arg(long)]
custom_prompt: Option<PathBuf>, // Load from file

#[arg(long, default_value = "en")]
target_language: String, // Translation target
```

### 3.3 Audio Preprocessing Options

**Problem**: No control over audio quality sent to API.

**Solution**:
```rust
#[arg(long, default_value = "2")]
audio_quality: u8, // ffmpeg -q:a value (0-9, lower is better)

#[arg(long)]
normalize: bool, // Apply audio normalization

#[arg(long)]
mono: bool, // Convert to mono (reduces size)

#[arg(long)]
sample_rate: Option<u32>, // Resample audio (e.g., 16000 for speech)
```

### 3.4 Configuration File Support

**Problem**: Many options to remember for repeated use.

**Solution**:
Support `.transcript-tool.toml` or `transcript-tool.json`:
```toml
[default]
model = "gemini-2.5-flash"
format = "json"
audio_quality = 2
parallel = 2

[api]
timeout_secs = 300
max_retries = 3
```

---

## Priority 4: Robustness & Production Readiness

### 4.1 Structured Error Types

**Problem**: All errors are stringly-typed through anyhow.

**Solution**:
```rust
#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error("API key not found in environment")]
    MissingApiKey,

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("File too large: {size} bytes (max: {max})")]
    FileTooLarge { size: u64, max: u64 },

    #[error("Gemini API error ({status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("ffmpeg failed: {0}")]
    FfmpegError(String),

    #[error("Invalid response from API")]
    InvalidResponse,
}
```

### 4.2 Graceful Shutdown

**Problem**: No handling of Ctrl+C during processing.

**Solution**:
```rust
use tokio::signal;

tokio::select! {
    result = process_file(&args) => result,
    _ = signal::ctrl_c() => {
        eprintln!("\nInterrupted. Cleaning up...");
        // Cleanup temp files
        Ok(())
    }
}
```

### 4.3 Checkpointing for Long Operations

**Problem**: Failure mid-processing requires full restart.

**Solution**:
- Write `.transcript-tool.checkpoint` file with progress
- On restart, detect checkpoint and offer to resume
- Useful for batch processing

### 4.4 Unit & Integration Tests

**Problem**: No tests.

**Solution**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_audio_file() {
        assert!(is_audio_file(Path::new("test.mp3")));
        assert!(is_audio_file(Path::new("test.WAV")));
        assert!(!is_audio_file(Path::new("test.mp4")));
    }

    #[test]
    fn test_get_mime_type() {
        assert_eq!(get_mime_type(Path::new("a.mp3")), "audio/mpeg");
        assert_eq!(get_mime_type(Path::new("a.wav")), "audio/wav");
    }

    #[tokio::test]
    async fn test_transcribe_mock() {
        // Use wiremock for API mocking
    }
}
```

---

## Priority 5: Advanced Features (Future)

### 5.1 Streaming for Large Files

Use Gemini File API for files > 20MB:
1. Upload file to File API
2. Reference file URI in generateContent
3. Delete file after processing

### 5.2 Real-time Transcription

Support live audio input via microphone or stream.

### 5.3 Speaker Diarization Enhancement

Post-process to merge consecutive segments from same speaker.

### 5.4 Local Whisper Fallback

Fall back to local Whisper model when API unavailable.

### 5.5 GUI/TUI Interface

Add optional terminal UI with `ratatui` for interactive use.

---

## Implementation Roadmap

| Phase | Items | Effort |
|-------|-------|--------|
| 1 | 1.1, 1.2, 1.3, 1.4 | Small |
| 2 | 2.1, 2.2, 2.3, 2.4 | Medium |
| 3 | 3.1, 3.2, 4.4 | Medium |
| 4 | 3.3, 3.4, 4.1, 4.2 | Medium |
| 5 | 4.3, 5.x | Large |

---

## Dependencies to Add

```toml
# Priority 1-2
indicatif = "0.17"          # Progress bars
tracing = "0.1"             # Structured logging
tracing-subscriber = "0.3"  # Log output

# Priority 3-4
thiserror = "2"             # Error types
toml = "0.8"                # Config file parsing
```
