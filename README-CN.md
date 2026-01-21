# Transcript Tool

一个用 Rust 编写的命令行工具，可从视频文件中提取音频并使用 Gemini API 生成详细的转录文本。

## 功能特性

- 使用 ffmpeg 从视频文件中提取音频
- 使用 Google Gemini 2.5 Flash API 进行音频转录
- 自动识别说话人
- 为每个片段生成时间戳
- 语言检测并支持英文翻译
- 情感检测（开心、悲伤、愤怒、中性）
- 结构化 JSON 输出

## 前置要求

- [Rust](https://rustup.rs/)（2024 版本）
- [ffmpeg](https://ffmpeg.org/) 已安装并配置在 PATH 中
- 从 [Google AI Studio](https://aistudio.google.com/) 获取的 Gemini API 密钥

## 安装

```bash
git clone <repository-url>
cd transcript_tool
cargo build --release
```

编译后的二进制文件位于 `target/release/convert`。

## 配置

将 Gemini API 密钥设置为环境变量：

```bash
export GEMINI_API_KEY="your-api-key"
# 或者
export GOOGLE_AI_KEY="your-api-key"
```

## 使用方法

```bash
# 基本用法 - 将视频转换为转录文本
convert -i video.mp4

# 直接输入音频文件（跳过 ffmpeg 提取步骤）
convert -i audio.mp3

# 指定自定义输出路径
convert -i video.mp4 -o transcript.json

# 保留中间生成的 MP3 文件
convert -i video.mp4 --keep-audio
```

### 命令行选项

| 选项 | 简写 | 描述 |
|------|------|------|
| `--input` | `-i` | 输入的视频或音频文件路径（必填） |
| `--output` | `-o` | 输出的 JSON 文件路径（默认为 `<input>.json`） |
| `--keep-audio` | `-k` | 保留中间生成的 MP3 文件 |
| `--help` | `-h` | 显示帮助信息 |

## 输出格式

工具生成的 JSON 文件结构如下：

```json
{
  "summary": "音频内容的简要概述。",
  "segments": [
    {
      "speaker": "Speaker 1",
      "timestamp": "00:05",
      "content": "转录的文本内容...",
      "language": "English",
      "language_code": "en",
      "translation": null,
      "emotion": "neutral"
    },
    {
      "speaker": "Speaker 2",
      "timestamp": "00:15",
      "content": "另一个片段...",
      "language": "Chinese",
      "language_code": "zh",
      "translation": "这里是英文翻译",
      "emotion": "happy"
    }
  ]
}
```

### 字段说明

| 字段 | 描述 |
|------|------|
| `summary` | 整个音频内容的简要概述 |
| `speaker` | 识别的说话人（如 "Speaker 1"、"Host"、"Guest"） |
| `timestamp` | 时间位置，格式为 MM:SS |
| `content` | 转录的文本 |
| `language` | 检测到的语言名称 |
| `language_code` | ISO 语言代码 |
| `translation` | 英文翻译（如果内容不是英文） |
| `emotion` | 检测到的情感：happy（开心）、sad（悲伤）、angry（愤怒）或 neutral（中性） |

## 支持的格式

### 输入视频格式
ffmpeg 支持的任何格式（mp4、mkv、avi、mov、webm 等）

### 输入音频格式
mp3、wav、ogg、flac、m4a、aac、wma

## 许可证

MIT
