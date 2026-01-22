implement a imagen gen rust cli, input promp text or yaml call `gemini-2.5-flash-image` or `gemini-3-pro-image-preview` model generate image

gemini 2.5 api format:
```bash
curl -s -X POST \
  "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-image:generateContent" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{
      "parts": [
        {"text": "Create a picture of a nano banana dish in a fancy restaurant with a Gemini theme"}
      ]
    }]
  }'
```

gemini 3 pro api format:
```
curl -s -X POST \
  "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-image-preview:generateContent" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{"parts": [{"text": "Da Vinci style anatomical sketch of a dissected Monarch butterfly. Detailed drawings of the head, wings, and legs on textured parchment with notes in English."}]}],
    "tools": [{"google_search": {}}],
    "generationConfig": {
      "responseModalities": ["TEXT", "IMAGE"],
      "imageConfig": {"aspectRatio": "1:1", "imageSize": "1K"}
    }
  }'
```
Gemini 3 Pro Image generates 1K images by default but can also output 2K and 4K images. To generate higher resolution assets, specify the image_size in the generation_config.

You must use an uppercase 'K' (e.g., 1K, 2K, 4K). Lowercase parameters (e.g., 1k) will be rejected.



yaml:
```yaml
prompts:
  - name: memory-safety
    prompt: |
      technical illustration explaining Rust memory safety:
        - Peaceful magical lab with animated arrows showing data flow between stack and heap
        - Characters representing Ownership, Borrow (&), Mutable Borrow (&mut), and Lifetime ('a)
        - A friendly compile-time checker character preventing dangling pointers and use-after-free
        - Warm colors, soft glowing labels: "Stack", "Heap", "Ownership", "&", "&mut", "'a"
        - Wide composition, lush background but with clear technical annotations
  - name: concurrency-safety
    prompt: |
      diagram illustrating Rust's concurrency safety:
      - Two cozy threads depicted as lively creatures exchanging data via an Arc<Mutex<T>> chest
      - Magical lock representing Mutex, glowing channel pipes for Send/Sync
      - A guardian spirit showing "no data races" shield at compile time
      - Labels: "Thread 1", "Thread 2", "Arc<Mutex<T>>", "Send", "Sync"
      - Wide, bright, friendly, but technically accurate
```
