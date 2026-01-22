# 0005 - Imagen CLI Implementation Plan

## Overview

Implement a Rust CLI tool for generating images using Google's Gemini image generation models. The tool will support both direct text prompts and YAML configuration files for batch prompt processing.

## Supported Models

| Model | Endpoint | Default Size | Sizes Available |
|-------|----------|--------------|-----------------|
| `gemini-2.5-flash-image` | `/v1beta/models/gemini-2.5-flash-image:generateContent` | - | Standard |
| `gemini-3-pro-image-preview` | `/v1beta/models/gemini-3-pro-image-preview:generateContent` | 1K | 1K, 2K, 4K |

## CLI Design

### Binary Name
`imagen` (new binary alongside `convert` and `batch_convert`)

### Usage Examples

```bash
# Direct text prompt (default model: gemini-2.5-flash-image)
imagen "A sunset over mountains"

# Specify model
imagen --model gemini-3-pro "A futuristic cityscape"
imagen -m 3pro "A futuristic cityscape"

# Specify output file
imagen "A cat" -o cat.png

# Specify image size (Gemini 3 Pro only)
imagen -m 3pro --size 2K "High resolution landscape"

# Specify aspect ratio (Gemini 3 Pro only)
imagen -m 3pro --aspect 16:9 "Wide panorama"

# From YAML file
imagen --yaml prompts.yaml

# From YAML with specific prompt name
imagen --yaml prompts.yaml --name memory-safety

# Verbose output
imagen -v "A prompt"
```

### CLI Arguments

```
USAGE:
    imagen [OPTIONS] [PROMPT]

ARGS:
    <PROMPT>    Text prompt for image generation (required unless --yaml is used)

OPTIONS:
    -m, --model <MODEL>      Model to use [default: 2.5-flash]
                             Values: 2.5-flash, 3pro
    -o, --output <FILE>      Output filename [default: generated from prompt or name]
    -s, --size <SIZE>        Image size (Gemini 3 Pro only) [default: 1K]
                             Values: 1K, 2K, 4K
    -a, --aspect <RATIO>     Aspect ratio (Gemini 3 Pro only) [default: 1:1]
                             Values: 1:1, 16:9, 9:16, 4:3, 3:4
    -y, --yaml <FILE>        YAML file with prompts
    -n, --name <NAME>        Specific prompt name from YAML (generates one image)
                             If omitted with --yaml, generates all prompts
    -d, --output-dir <DIR>   Output directory [default: current directory]
    -v, --verbose            Increase verbosity
    -h, --help               Print help
    -V, --version            Print version
```

## Module Structure

```
src/
├── imagen.rs           # CLI binary entry point
├── imagen_api.rs       # Gemini Image API client
└── lib.rs              # Add module exports
```

### imagen_api.rs

```rust
// Model enum
pub enum ImageModel {
    Gemini25Flash,      // gemini-2.5-flash-image
    Gemini3Pro,         // gemini-3-pro-image-preview
}

// Image size (Gemini 3 Pro only)
pub enum ImageSize {
    K1,     // "1K"
    K2,     // "2K"
    K4,     // "4K"
}

// Aspect ratio (Gemini 3 Pro only)
pub enum AspectRatio {
    Square,     // "1:1"
    Wide,       // "16:9"
    Tall,       // "9:16"
    Standard,   // "4:3"
    Portrait,   // "3:4"
}

// Request configuration
pub struct ImageGenConfig {
    pub model: ImageModel,
    pub size: Option<ImageSize>,        // Gemini 3 Pro only
    pub aspect_ratio: Option<AspectRatio>, // Gemini 3 Pro only
}

// Response types
pub struct ImageGenResponse {
    pub image_data: Vec<u8>,    // Decoded image bytes
    pub mime_type: String,      // e.g., "image/png"
}

// Main API function
pub async fn generate_image(
    prompt: &str,
    config: &ImageGenConfig,
) -> Result<ImageGenResponse, ImageGenError>;
```

### YAML Format

```yaml
prompts:
  - name: memory-safety           # Required: unique identifier
    prompt: |                     # Required: the prompt text
      technical illustration...
    model: 3pro                   # Optional: override default model
    size: 2K                      # Optional: Gemini 3 Pro only
    aspect: 16:9                  # Optional: Gemini 3 Pro only
    output: memory-safety.png    # Optional: custom output filename
```

## API Implementation Details

### Gemini 2.5 Flash Request

```json
{
  "contents": [{
    "parts": [
      {"text": "<prompt>"}
    ]
  }]
}
```

### Gemini 3 Pro Request

```json
{
  "contents": [{"parts": [{"text": "<prompt>"}]}],
  "generationConfig": {
    "responseModalities": ["TEXT", "IMAGE"],
    "imageConfig": {
      "aspectRatio": "1:1",
      "imageSize": "1K"
    }
  }
}
```

### Response Parsing

The response contains base64-encoded image data in the `inlineData` field:

```json
{
  "candidates": [{
    "content": {
      "parts": [{
        "inlineData": {
          "mimeType": "image/png",
          "data": "<base64-encoded-image>"
        }
      }]
    }
  }]
}
```

## Error Handling

- **API key missing**: Check `GEMINI_API_KEY` or `GOOGLE_AI_KEY`
- **Rate limiting (429)**: Retry with exponential backoff (reuse existing retry logic)
- **Invalid model**: Validate model name before request
- **Invalid size/aspect**: Validate parameters, warn if used with 2.5-flash
- **YAML parse errors**: Clear error messages with line numbers
- **Network errors**: Retry with backoff
- **Image decode errors**: Report and continue (for batch)

## Implementation Steps

1. **Create `imagen_api.rs`**
   - Define enums: `ImageModel`, `ImageSize`, `AspectRatio`
   - Define structs: `ImageGenConfig`, `ImageGenResponse`
   - Implement `generate_image()` async function
   - Reuse HTTP client setup from `gemini_api.rs`
   - Implement response parsing and base64 decoding

2. **Create `imagen.rs` (CLI binary)**
   - Define CLI args with clap
   - Implement direct prompt mode
   - Implement YAML parsing with serde
   - Implement single prompt selection (`--name`)
   - Implement batch generation from YAML
   - Add progress indication

3. **Update `lib.rs`**
   - Export `imagen_api` module

4. **Update `Cargo.toml`**
   - Add `[[bin]]` entry for `imagen`
   - Ensure dependencies: `clap`, `serde`, `serde_yaml`, `tokio`, `reqwest`, `base64`

5. **Testing**
   - Unit tests for request building
   - Unit tests for response parsing
   - Unit tests for YAML parsing
   - Integration test with mock server (optional)

## Dependencies

Already in project:
- `clap` - CLI parsing
- `tokio` - async runtime
- `reqwest` - HTTP client
- `serde`, `serde_json` - JSON serialization
- `base64` - base64 encoding/decoding

May need to add:
- `serde_yaml` - YAML parsing

## Output Filename Generation

When no explicit output is specified:
1. If `--name` is used: `{name}.png`
2. If direct prompt: slugify first 50 chars of prompt + `.png`
3. If YAML batch: use `name` field from each prompt

## Future Enhancements (Out of Scope)

- Image-to-image generation (input image + prompt)
- Multiple images per prompt
- Interactive mode
- Streaming progress for large images
