# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Transcript Tool is a Rust CLI application that extracts audio from video files and generates transcripts using Google's Gemini API. It supports speaker identification, timestamps, emotion detection, language detection with translation, and multiple output formats (JSON, SRT, VTT, TXT).

## Build Commands

```bash
# Build
cargo build                    # Development build
cargo build --release          # Optimized release build

# Test
cargo test --all-features      # Run all tests
cargo test <test_name>         # Run single test

# Lint and format
cargo fmt --all -- --check     # Check formatting
cargo clippy --all-targets --all-features --tests --benches -- -D warnings
```

## Running the Tools

Requires `GEMINI_API_KEY` or `GOOGLE_AI_KEY` environment variable.

### Single File (`convert`)
```bash
./target/release/convert -i video.mp4 -f srt
./target/release/convert -i audio.mp3 -f json -vv --timeout 600
```

### Batch Processing (`batch_convert`)
```bash
./target/release/batch_convert /path/to/folder -f json -j 4
./target/release/batch_convert folder1 folder2 -f srt --jobs 2 -v
```
- Recursively finds all video/audio files in folders
- Processes files in parallel (`-j` controls concurrency)
- Continues on errors, reports failures at end

## Architecture

```
src/
├── convert.rs       # Single file CLI (binary: "convert")
├── batch_convert.rs # Batch processing CLI (binary: "batch_convert")
├── gemini_api.rs    # Gemini API client with retry logic
├── file_api.rs      # Large file upload (>20MB) via Gemini File API
└── lib.rs           # Library exports for shared code
```

**Data Flow:**
1. Parse CLI args (clap)
2. Extract audio via ffmpeg (if video input)
3. Size check: ≤20MB uses inline base64, >20MB uses File API resumable upload
4. Send to Gemini API with retry logic (exponential backoff)
5. Parse response into TranscriptResponse
6. Convert to output format and write file
7. Cleanup temp files

**Key Constants:**
- `MAX_INLINE_FILE_SIZE`: 20MB (files larger use File API)
- `DEFAULT_TIMEOUT_SECS`: 600 (10 minutes)
- `DEFAULT_MAX_RETRIES`: 3

**Retry Logic:** Retries on 429 (rate limit), network errors, and 5xx errors with exponential backoff (2^n seconds).

## Key Types

```rust
// gemini_api.rs
struct TranscriptResponse { summary, segments }
struct TranscriptSegment { speaker, timestamp, content, language, language_code, translation, emotion }
enum AudioSource { Inline{mime_type, data}, FileUri{mime_type, uri} }

// file_api.rs
struct FileInfo { name, uri, mime_type, size_bytes, state, display_name }

// convert.rs
enum OutputFormat { Json, Srt, Vtt, Txt }
```

## Testing

Tests are inline in each module using `#[test]` and `#[tokio::test]`:
- `convert.rs`: Format conversion, timestamp formatting, audio file detection
- `batch_convert.rs`: Media file detection, output extension mapping
- `gemini_api.rs`: MIME type detection, file size validation, base64 encoding
- `file_api.rs`: FileInfo deserialization, file ID extraction
