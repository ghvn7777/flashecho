# Transcript Tool

[English](README.md) | [中文](README-CN.md)

一个用 Rust 编写的命令行工具，可从视频文件中提取音频并使用 Gemini API 生成详细的转录文本。

## 功能特性

- 使用 ffmpeg 从视频文件中提取音频
- 使用 Google Gemini 2.5 Flash API 进行音频转录
- 自动识别说话人
- 为每个片段生成时间戳
- 语言检测并支持英文翻译
- 情感检测（开心、悲伤、愤怒、中性）
- 多种输出格式：JSON、SRT、VTT、TXT
- 带旋转动画的进度指示
- 可配置的指数退避重试逻辑
- 文件大小验证（内联数据最大 20MB）
- 自动 MIME 类型检测
- 多级详细日志

## 前置要求

- [Rust](https://rustup.rs/)（2024 版本）
- [ffmpeg](https://ffmpeg.org/) 已安装并配置在 PATH 中
- 从 [Google AI Studio](https://aistudio.google.com/) 获取的 Gemini API 密钥

## 安装

```bash
git clone https://github.com/ghvn7777/flashecho.git
cd transcript_tool
cargo build --release
```

编译后的二进制文件位于 `target/release/convert`。

## 配置

将 Gemini API 密钥设置为环境变量：

```bash
export GEMINI_API_KEY="your-api-key"
# 或者
# export GOOGLE_AI_KEY="your-api-key"
```

## 使用方法

```bash
# 基本用法 - 将视频转换为 JSON 转录文本
convert -i video.mp4

# 直接输入音频文件（跳过 ffmpeg 提取步骤）
convert -i audio.mp3

# 输出为 SRT 字幕格式
convert -i video.mp4 -f srt

# 输出为 WebVTT 字幕格式
convert -i video.mp4 -f vtt

# 输出为纯文本格式
convert -i video.mp4 -f txt

# 指定自定义输出路径
convert -i video.mp4 -o transcript.json

# 保留中间生成的 MP3 文件
convert -i video.mp4 --keep-audio

# 使用不同的 Gemini 模型
convert -i video.mp4 --model gemini-2.0-flash

# 增加 API 超时时间（默认：300 秒）
convert -i video.mp4 --timeout 600

# 设置最大重试次数（默认：3）
convert -i video.mp4 --max-retries 5

# 启用详细日志
convert -i video.mp4 -v      # INFO 级别
convert -i video.mp4 -vv     # DEBUG 级别
convert -i video.mp4 -vvv    # TRACE 级别

# 安静模式（无进度输出）
convert -i video.mp4 -q
```

### 命令行选项

| 选项 | 简写 | 描述 | 默认值 |
|------|------|------|--------|
| `--input` | `-i` | 输入的视频或音频文件路径 | （必填） |
| `--output` | `-o` | 输出文件路径 | `<input>.<format>` |
| `--format` | `-f` | 输出格式 (json, srt, vtt, txt) | `json` |
| `--keep-audio` | `-k` | 保留中间生成的 MP3 文件 | `false` |
| `--model` | | 使用的 Gemini 模型 | `gemini-2.5-flash` |
| `--timeout` | | API 超时时间（秒） | `300` |
| `--max-retries` | | API 调用最大重试次数 | `3` |
| `--verbose` | `-v` | 详细程度 (-v, -vv, -vvv) | warn |
| `--quiet` | `-q` | 安静模式（无进度输出） | `false` |
| `--help` | `-h` | 显示帮助信息 | |
| `--version` | `-V` | 显示版本 | |

## 输出格式

### JSON（默认）

带完整元数据的结构化 JSON：

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
    }
  ]
}
```

### SRT（SubRip 字幕）

用于视频播放器的标准字幕格式：

```
1
00:00:05,000 --> 00:00:10,000
[Speaker 1] 你好，欢迎来到节目。

2
00:00:10,000 --> 00:00:15,000
[Speaker 2] 感谢邀请我。
```

### VTT（WebVTT）

网页友好的字幕格式：

```
WEBVTT

00:00:05.000 --> 00:00:10.000
<v Speaker 1>你好，欢迎来到节目。

00:00:10.000 --> 00:00:15.000
<v Speaker 2>感谢邀请我。
```

### TXT（纯文本）

人类可读的纯文本格式：

```
Summary:
两位说话人之间关于...的对话

---

[00:05] Speaker 1 (neutral)
你好，欢迎来到节目。

[00:10] Speaker 2 (happy)
感谢邀请我。
```

## 支持的格式

### 输入视频格式
ffmpeg 支持的任何格式（mp4、mkv、avi、mov、webm 等）

### 输入音频格式
mp3、wav、ogg、flac、m4a、aac、wma、webm

## 错误处理

工具包含健壮的错误处理：

- **重试逻辑**：网络错误和服务器错误（5xx）时自动使用指数退避重试
- **速率限制**：检测 429 响应并适当重试
- **文件大小验证**：上传大于 20MB 的文件前发出警告
- **超时配置**：可为长音频文件配置超时时间

## 许可证

MIT
