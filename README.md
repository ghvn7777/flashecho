# Transcript Tool

[中文](!./README-CN.md)

A CLI tool written in Rust that extracts audio from video files and generates detailed transcripts using the Gemini API.

## Features

- Extract audio from video files using ffmpeg
- Transcribe audio using Google's Gemini 2.5 Flash API
- Automatic speaker identification
- Timestamp generation for each segment
- Language detection with English translation support
- Emotion detection (happy, sad, angry, neutral)
- Multiple output formats: JSON, SRT, VTT, TXT
- Progress indication with spinners
- Configurable retry logic with exponential backoff
- File size validation (max 20MB for inline data)
- Automatic MIME type detection
- Verbose logging levels

## Prerequisites

- [Rust](https://rustup.rs/) (2024 edition)
- [ffmpeg](https://ffmpeg.org/) installed and available in PATH
- Gemini API key from [Google AI Studio](https://aistudio.google.com/)

## Installation

```bash
git clone <repository-url>
cd transcript_tool
cargo build --release
```

The binary will be available at `target/release/convert`.

## Configuration

Set your Gemini API key as an environment variable:

```bash
export GEMINI_API_KEY="your-api-key"
# or
export GOOGLE_AI_KEY="your-api-key"
```

## Usage

```bash
# Basic usage - convert video to JSON transcript
convert -i video.mp4

# Input an audio file directly (skips ffmpeg extraction)
convert -i audio.mp3

# Output as SRT subtitles
convert -i video.mp4 -f srt

# Output as WebVTT subtitles
convert -i video.mp4 -f vtt

# Output as plain text
convert -i video.mp4 -f txt

# Specify custom output path
convert -i video.mp4 -o transcript.json

# Keep the intermediate MP3 file
convert -i video.mp4 --keep-audio

# Use a different Gemini model
convert -i video.mp4 --model gemini-2.0-flash

# Increase API timeout (default: 300 seconds)
convert -i video.mp4 --timeout 600

# Set max retry attempts (default: 3)
convert -i video.mp4 --max-retries 5

# Enable verbose logging
convert -i video.mp4 -v      # INFO level
convert -i video.mp4 -vv     # DEBUG level
convert -i video.mp4 -vvv    # TRACE level

# Quiet mode (no progress output)
convert -i video.mp4 -q
```

### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input` | `-i` | Input video or audio file path | (required) |
| `--output` | `-o` | Output file path | `<input>.<format>` |
| `--format` | `-f` | Output format (json, srt, vtt, txt) | `json` |
| `--keep-audio` | `-k` | Keep the intermediate MP3 file | `false` |
| `--model` | | Gemini model to use | `gemini-2.5-flash` |
| `--timeout` | | API timeout in seconds | `300` |
| `--max-retries` | | Max retry attempts for API calls | `3` |
| `--verbose` | `-v` | Verbosity level (-v, -vv, -vvv) | warn |
| `--quiet` | `-q` | Quiet mode (no progress output) | `false` |
| `--help` | `-h` | Print help information | |
| `--version` | `-V` | Print version | |

## Output Formats

### JSON (default)

Structured JSON with full metadata:

```json
{
  "summary": "A concise summary of the audio content.",
  "segments": [
    {
      "speaker": "Speaker 1",
      "timestamp": "00:05",
      "content": "Transcribed text content...",
      "language": "English",
      "language_code": "en",
      "translation": null,
      "emotion": "neutral"
    }
  ]
}
```

### SRT (SubRip Subtitles)

Standard subtitle format for video players:

```
1
00:00:05,000 --> 00:00:10,000
[Speaker 1] Hello, welcome to the show.

2
00:00:10,000 --> 00:00:15,000
[Speaker 2] Thanks for having me.
```

### VTT (WebVTT)

Web-friendly subtitle format:

```
WEBVTT

00:00:05.000 --> 00:00:10.000
<v Speaker 1>Hello, welcome to the show.

00:00:10.000 --> 00:00:15.000
<v Speaker 2>Thanks for having me.
```

### TXT (Plain Text)

Human-readable plain text format:

```
Summary:
A conversation between two speakers discussing...

---

[00:05] Speaker 1 (neutral)
Hello, welcome to the show.

[00:10] Speaker 2 (happy)
Thanks for having me.
```

## Supported Formats

### Input Video Formats
Any format supported by ffmpeg (mp4, mkv, avi, mov, webm, etc.)

### Input Audio Formats
mp3, wav, ogg, flac, m4a, aac, wma, webm

## Error Handling

The tool includes robust error handling:

- **Retry Logic**: Automatically retries on network errors and server errors (5xx) with exponential backoff
- **Rate Limiting**: Detects 429 responses and retries appropriately
- **File Size Validation**: Warns before uploading files larger than 20MB
- **Timeout Configuration**: Configurable timeout for long audio files

## License

MIT
