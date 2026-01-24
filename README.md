# Transcript Tool

[English](README.md) | [中文](README-CN.md)

A CLI toolset written in Rust for media processing with Google's Gemini API:
- **Audio Transcription** - Extract audio from video files and generate detailed transcripts
- **Image Generation** - Generate images from text prompts using Gemini image models
- **Image Editing** - Edit and transform images with text prompts using Gemini 3 Pro

## Features

### Transcription (`convert`, `batch_convert`)
- Extract audio from video files using ffmpeg
- Transcribe audio using Google's Gemini 2.5 Flash API
- **Batch processing** - process entire folders recursively
- **Skip existing transcripts** - automatically skip files that already have transcripts
- Automatic speaker identification
- Timestamp generation for each segment
- Language detection with English translation support
- Emotion detection (happy, sad, angry, neutral)
- Multiple output formats: JSON, SRT, VTT, TXT
- **Large file support** - files >20MB automatically use Gemini File API (up to 2GB)
- Progress indication with spinners
- Configurable retry logic with exponential backoff
- Smart rate limit handling with longer backoff for 429 errors
- Input format validation
- Automatic MIME type detection
- Verbose logging levels

### Image Generation (`imagen`)
- Generate images using Gemini 2.5 Flash Image or Gemini 3 Pro Image models
- Support for text prompts or YAML batch files
- Configurable image size (1K, 2K, 4K) and aspect ratio (Gemini 3 Pro)
- Parallel image generation with semaphore-based concurrency control
- Output filenames with slug + hash format for uniqueness

### Image Editing (`imagen_edit`)
- Edit and transform images using Gemini 3 Pro Image model
- Support for multiple input images (e.g., combine faces into group photo)
- CLI mode for single edits or YAML batch files
- Configurable image size (1K, 2K, 4K) and aspect ratio
- Parallel processing with semaphore-based concurrency control
- Image paths in YAML resolved relative to YAML file location

## Prerequisites

- [Rust](https://rustup.rs/) (2024 edition)
- [ffmpeg](https://ffmpeg.org/) installed and available in PATH
- Gemini API key from [Google AI Studio](https://aistudio.google.com/)

## Installation

```bash
git clone https://github.com/ghvn7777/flashecho.git
cd transcript_tool
cargo build --release
```

The binaries will be available at:
- `target/release/convert` - Single file transcription
- `target/release/batch_convert` - Batch transcription
- `target/release/imagen` - Image generation
- `target/release/imagen_edit` - Image editing

## Configuration

Set your Gemini API key as an environment variable:

```bash
export GEMINI_API_KEY="your-api-key"
# or
# export GOOGLE_AI_KEY="your-api-key"
```

## Usage

### Single File (`convert`)

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

# Increase API timeout (default: 600 seconds)
convert -i video.mp4 --timeout 900

# Set max retry attempts (default: 3)
convert -i video.mp4 --max-retries 5

# Enable verbose logging
convert -i video.mp4 -v      # INFO level
convert -i video.mp4 -vv     # DEBUG level
convert -i video.mp4 -vvv    # TRACE level

# Quiet mode (no progress output)
convert -i video.mp4 -q
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input` | `-i` | Input video or audio file path | (required) |
| `--output` | `-o` | Output file path | `<input>.<format>` |
| `--format` | `-f` | Output format (json, srt, vtt, txt) | `json` |
| `--keep-audio` | `-k` | Keep the intermediate MP3 file | `false` |
| `--model` | | Gemini model to use | `gemini-2.5-flash` |
| `--timeout` | | API timeout in seconds | `600` |
| `--max-retries` | | Max retry attempts for API calls | `3` |
| `--force-file-api` | | Force File API even for small files | `false` |
| `--keep-remote-file` | | Keep uploaded file on server | `false` |
| `--verbose` | `-v` | Verbosity level (-v, -vv, -vvv) | warn |
| `--quiet` | `-q` | Quiet mode (no progress output) | `false` |
| `--help` | `-h` | Print help information | |
| `--version` | `-V` | Print version | |

### Batch Processing (`batch_convert`)

Process multiple files from one or more folders recursively.

```bash
# Process all media files in a folder
batch_convert /path/to/folder

# Process multiple folders
batch_convert folder1 folder2 folder3

# Output as SRT format
batch_convert /path/to/folder -f srt

# Control parallel jobs (default: 2)
batch_convert /path/to/folder -j 4

# Adjust delay between tasks to avoid rate limiting (default: 5 seconds)
batch_convert /path/to/folder -d 10

# Conservative settings for strict rate limits
batch_convert /path/to/folder -j 1 -d 10

# Enable verbose logging
batch_convert /path/to/folder -v
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `FOLDERS` | | Folder paths to process (recursive) | (required) |
| `--format` | `-f` | Output format (json, srt, vtt, txt) | `json` |
| `--jobs` | `-j` | Number of parallel jobs | `2` |
| `--delay` | `-d` | Delay in seconds between starting tasks | `5` |
| `--keep-audio` | `-k` | Keep the intermediate MP3 files | `false` |
| `--model` | | Gemini model to use | `gemini-2.5-flash` |
| `--timeout` | | API timeout in seconds | `600` |
| `--max-retries` | | Max retry attempts for API calls | `3` |
| `--force-file-api` | | Force File API even for small files | `false` |
| `--keep-remote-file` | | Keep uploaded file on server | `false` |
| `--verbose` | `-v` | Verbosity level (-v, -vv, -vvv) | warn |
| `--quiet` | `-q` | Quiet mode (no progress output) | `false` |
| `--help` | `-h` | Print help information | |
| `--version` | `-V` | Print version | |

### Image Generation (`imagen`)

Generate images from text prompts using Gemini image models.

```bash
# Basic usage - generate image from prompt
imagen "A sunset over mountains"

# Use Gemini 3 Pro model (supports size/aspect options)
imagen -m 3pro "A futuristic cityscape"

# High resolution with custom aspect ratio (Gemini 3 Pro only)
imagen -m 3pro --size 2K --aspect 16:9 "Wide panorama landscape"

# Specify output file
imagen "A cat" -o cat.png

# Generate from YAML file
imagen --yaml prompts.yaml

# Generate specific prompt from YAML
imagen --yaml prompts.yaml --name memory-safety

# Parallel generation with 4 jobs
imagen --yaml prompts.yaml -j 4

# Quiet mode
imagen --yaml prompts.yaml -q
```

#### YAML Format

```yaml
prompts:
  - name: sunset
    prompt: A beautiful sunset over mountains
  - name: cityscape
    prompt: A futuristic cityscape at night
    model: 3pro        # Optional: override model
    size: 2K           # Optional: Gemini 3 Pro only
    aspect: 16:9       # Optional: Gemini 3 Pro only
    output: city.png   # Optional: custom filename
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `PROMPT` | | Text prompt for image generation | |
| `--yaml` | `-y` | YAML file containing prompts | |
| `--name` | `-n` | Generate specific prompt from YAML | |
| `--output` | `-o` | Output file/directory | `./output` |
| `--model` | `-m` | Model: `2.5-flash`, `3pro` | `2.5-flash` |
| `--size` | `-s` | Image size: `1K`, `2K`, `4K` (3pro only) | `1K` |
| `--aspect` | `-a` | Aspect ratio (3pro only) | `1:1` |
| `--jobs` | `-j` | Parallel jobs for YAML batch | `2` |
| `--timeout` | `-t` | API timeout in seconds | `120` |
| `--max-retries` | | Max retry attempts | `3` |
| `--verbose` | `-v` | Verbosity level (-v, -vv, -vvv) | warn |
| `--quiet` | `-q` | Quiet mode (no progress output) | `false` |
| `--help` | `-h` | Print help information | |
| `--version` | `-V` | Print version | |

#### Supported Models

| Model | Flag | Features |
|-------|------|----------|
| Gemini 2.5 Flash Image | `-m 2.5-flash` | Fast generation |
| Gemini 3 Pro Image | `-m 3pro` | Size (1K/2K/4K), aspect ratio options |

#### Aspect Ratios (Gemini 3 Pro)

- `1:1` - Square (default)
- `16:9` - Wide/landscape
- `9:16` - Tall/portrait
- `4:3` - Standard
- `3:4` - Portrait

### Image Editing (`imagen_edit`)

Edit and transform images using Gemini 3 Pro model with text prompts.

```bash
# Single image edit
imagen_edit -i photo.jpg "Make it look like a watercolor painting"

# Multiple input images (e.g., combine faces into group photo)
imagen_edit -i face1.png -i face2.png -i face3.png "An office group photo of these people"

# With size and aspect ratio options
imagen_edit -i img1.jpg -i img2.jpg --size 2K --aspect 16:9 "Combine into panorama"

# Specify output file
imagen_edit -i portrait.png -o edited.png "Add a sunset background"

# Batch mode with YAML file
imagen_edit --yaml edits.yaml

# Process specific entry from YAML
imagen_edit --yaml edits.yaml --name group-photo

# Parallel processing with 4 jobs
imagen_edit --yaml edits.yaml -j 4

# Custom output directory
imagen_edit --yaml edits.yaml -o ./results
```

#### YAML Format

```yaml
edits:
  - name: group-photo
    prompt: An office group photo of these people, they are making funny faces
    images:
      - face1.png
      - face2.png
      - face3.png
    output: group.png   # Optional: custom filename
  - name: watercolor
    prompt: Make it look like a watercolor painting
    images:
      - photo.jpg
  - name: panorama
    prompt: Combine into a wide panorama
    images:
      - img1.jpg
      - img2.jpg
    size: 2K            # Optional
    aspect: 16:9        # Optional
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input` | `-i` | Input image file(s), can specify multiple | |
| `PROMPT` | | Text prompt describing the edit | |
| `--yaml` | `-y` | YAML file containing edit tasks | |
| `--name` | `-n` | Process specific entry from YAML | |
| `--output` | `-o` | Output file/directory | `./output` |
| `--size` | `-s` | Image size: `1K`, `2K`, `4K` | `1K` |
| `--aspect` | `-a` | Aspect ratio | `1:1` |
| `--jobs` | `-j` | Parallel jobs for YAML batch | `2` |
| `--timeout` | `-t` | API timeout in seconds | `120` |
| `--max-retries` | | Max retry attempts | `3` |
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
mp4, mkv, avi, mov, webm, flv, wmv, m4v

### Input Audio Formats
mp3, wav, ogg, flac, m4a, aac, wma

## Smart Features

- **Skip Existing**: Both `convert` and `batch_convert` automatically skip files that already have transcript output files
- **Input Validation**: Validates that input files are supported media formats and input paths are directories (for batch_convert)
- **Large File Support**: Files larger than 20MB automatically use the Gemini File API with resumable uploads (supports up to 2GB)

## Error Handling

The tool includes robust error handling:

- **Retry Logic**: Automatically retries on network errors and server errors (5xx) with exponential backoff
- **Smart Rate Limiting**: Detects 429 responses and uses longer backoff (30s, 60s, 90s) to avoid quota exhaustion
- **Batch Rate Control**: Use `--delay` and `--jobs` options to control API request rate in batch mode
- **Timeout Configuration**: Configurable timeout for long audio files (default: 10 minutes)

## License

MIT
