# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Transcript Tool is a Rust CLI toolset for media processing with Google's Gemini API:
- **Transcription**: Extracts audio from video files and generates transcripts with speaker identification, timestamps, emotion detection, language detection with translation, and multiple output formats (JSON, SRT, VTT, TXT).
- **Image Generation**: Generates images from text prompts using Gemini image models (2.5 Flash, 3 Pro) with support for batch processing, configurable sizes/aspect ratios, and parallel generation.
- **Image Editing**: Edits and transforms images with text prompts using Gemini 3 Pro, supporting multiple input images, YAML batch files, and parallel processing.

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

### Image Generation (`imagen`)
```bash
./target/release/imagen "A sunset over mountains"
./target/release/imagen -m 3pro --size 2K --aspect 16:9 "Wide panorama"
./target/release/imagen --yaml prompts.yaml -j 4
```
- Models: `2.5-flash` (default), `3pro` (supports size/aspect)
- Size options (3pro only): `1K`, `2K`, `4K`
- Aspect ratios (3pro only): `1:1`, `16:9`, `9:16`, `4:3`, `3:4`
- Parallel generation with `-j` flag

### Image Editing (`imagen_edit`)
```bash
./target/release/imagen_edit -i photo.jpg "Make it watercolor"
./target/release/imagen_edit -i face1.png -i face2.png "Group photo of these people"
./target/release/imagen_edit --yaml edits.yaml -j 4
```
- Multiple input images support (`-i` flag, can specify multiple)
- Size options: `1K`, `2K`, `4K`
- Aspect ratios: `1:1`, `16:9`, `9:16`, `4:3`, `3:4`
- YAML batch mode with `images` array per entry
- Parallel processing with `-j` flag

## Architecture

```
src/
├── convert.rs        # Single file CLI (binary: "convert")
├── batch_convert.rs  # Batch processing CLI (binary: "batch_convert")
├── imagen.rs         # Image generation CLI (binary: "imagen")
├── imagen_edit.rs    # Image editing CLI (binary: "imagen_edit")
├── gemini_api.rs     # Gemini API client for transcription
├── imagen_api.rs     # Gemini API client for image generation
├── imagen_edit_api.rs # Gemini API client for image editing
├── file_api.rs       # Large file upload (>20MB) via Gemini File API
└── lib.rs            # Library exports for shared code
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

// imagen_api.rs
enum ImageModel { Gemini25Flash, Gemini3Pro }
enum ImageSize { K1, K2, K4 }
enum AspectRatio { Square, Wide, Tall, Standard, Portrait }
struct ImageGenConfig { size, aspect_ratio }
struct GeneratedImage { data, mime_type }

// imagen_edit_api.rs
struct InputImage { mime_type, data }
struct ImageEditConfig { size, aspect_ratio }
struct ImageEditClient { client, api_key, config }
```

## Testing

Tests are inline in each module using `#[test]` and `#[tokio::test]`:
- `convert.rs`: Format conversion, timestamp formatting, audio file detection
- `batch_convert.rs`: Media file detection, output extension mapping
- `gemini_api.rs`: MIME type detection, file size validation, base64 encoding
- `file_api.rs`: FileInfo deserialization, file ID extraction
- `imagen_api.rs`: Model/size/aspect parsing, image extension mapping
- `imagen.rs`: YAML parsing, slugify, filename generation
- `imagen_edit_api.rs`: MIME type detection, input image handling, edit config
- `imagen_edit.rs`: YAML parsing, image path resolution, batch editing
