# Transcript Tool

A CLI tool written in Rust that extracts audio from video files and generates detailed transcripts using the Gemini API.

## Features

- Extract audio from video files using ffmpeg
- Transcribe audio using Google's Gemini 2.5 Flash API
- Automatic speaker identification
- Timestamp generation for each segment
- Language detection with English translation support
- Emotion detection (happy, sad, angry, neutral)
- Structured JSON output

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
# Basic usage - convert video to transcript
convert -i video.mp4

# Input an audio file directly (skips ffmpeg extraction)
convert -i audio.mp3

# Specify custom output path
convert -i video.mp4 -o transcript.json

# Keep the intermediate MP3 file
convert -i video.mp4 --keep-audio
```

### Options

| Option | Short | Description |
|--------|-------|-------------|
| `--input` | `-i` | Input video or audio file path (required) |
| `--output` | `-o` | Output JSON file path (defaults to `<input>.json`) |
| `--keep-audio` | `-k` | Keep the intermediate MP3 file |
| `--help` | `-h` | Print help information |

## Output Format

The tool generates a JSON file with the following structure:

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
    },
    {
      "speaker": "Speaker 2",
      "timestamp": "00:15",
      "content": "Another segment...",
      "language": "Chinese",
      "language_code": "zh",
      "translation": "English translation here",
      "emotion": "happy"
    }
  ]
}
```

### Fields

| Field | Description |
|-------|-------------|
| `summary` | Brief overview of the entire audio content |
| `speaker` | Identified speaker (e.g., "Speaker 1", "Host", "Guest") |
| `timestamp` | Time position in MM:SS format |
| `content` | Transcribed text |
| `language` | Detected language name |
| `language_code` | ISO language code |
| `translation` | English translation (if content is non-English) |
| `emotion` | Detected emotion: happy, sad, angry, or neutral |

## Supported Formats

### Input Video Formats
Any format supported by ffmpeg (mp4, mkv, avi, mov, webm, etc.)

### Input Audio Formats
mp3, wav, ogg, flac, m4a, aac, wma

## License

MIT
