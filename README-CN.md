# Transcript Tool

[English](README.md) | [中文](README-CN.md)

一套用 Rust 编写的命令行工具，使用 Google Gemini API 进行媒体处理：
- **音频转录** - 从视频文件中提取音频并生成详细的转录文本
- **图像生成** - 使用 Gemini 图像模型从文本提示生成图像
- **图像编辑** - 使用 Gemini 3 Pro 通过文本提示编辑和转换图像

## 功能特性

### 转录功能 (`convert`, `batch_convert`)
- 使用 ffmpeg 从视频文件中提取音频
- 使用 Google Gemini 2.5 Flash API 进行音频转录
- **批量处理** - 递归处理整个文件夹
- **跳过已有转录** - 自动跳过已有转录文件的媒体文件
- 自动识别说话人
- 为每个片段生成时间戳
- 语言检测并支持英文翻译
- 情感检测（开心、悲伤、愤怒、中性）
- 多种输出格式：JSON、SRT、VTT、TXT
- **大文件支持** - 超过 20MB 的文件自动使用 Gemini File API（最大支持 2GB）
- 带旋转动画的进度指示
- 可配置的指数退避重试逻辑
- 智能速率限制处理，对 429 错误使用更长的退避时间
- 输入格式验证
- 自动 MIME 类型检测
- 多级详细日志

### 图像生成 (`imagen`)
- 使用 Gemini 2.5 Flash Image 或 Gemini 3 Pro Image 模型生成图像
- 支持文本提示或 YAML 批量文件
- 可配置图像尺寸（1K、2K、4K）和宽高比（仅 Gemini 3 Pro）
- 基于信号量的并行图像生成
- 输出文件名使用 slug + 哈希格式确保唯一性

### 图像编辑 (`imagen_edit`)
- 使用 Gemini 3 Pro Image 模型编辑和转换图像
- 支持多张输入图像（例如：将多张人脸合成为合照）
- 命令行模式支持单次编辑或 YAML 批量文件
- 可配置图像尺寸（1K、2K、4K）和宽高比
- 基于信号量的并行处理
- YAML 中的图像路径相对于 YAML 文件位置解析

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

编译后的二进制文件位于：
- `target/release/convert` - 单文件转录
- `target/release/batch_convert` - 批量转录
- `target/release/imagen` - 图像生成
- `target/release/imagen_edit` - 图像编辑

## 配置

将 Gemini API 密钥设置为环境变量：

```bash
export GEMINI_API_KEY="your-api-key"
# 或者
# export GOOGLE_AI_KEY="your-api-key"
```

## 使用方法

### 单文件处理 (`convert`)

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

# 增加 API 超时时间（默认：600 秒）
convert -i video.mp4 --timeout 900

# 设置最大重试次数（默认：3）
convert -i video.mp4 --max-retries 5

# 启用详细日志
convert -i video.mp4 -v      # INFO 级别
convert -i video.mp4 -vv     # DEBUG 级别
convert -i video.mp4 -vvv    # TRACE 级别

# 安静模式（无进度输出）
convert -i video.mp4 -q
```

#### 命令行选项

| 选项 | 简写 | 描述 | 默认值 |
|------|------|------|--------|
| `--input` | `-i` | 输入的视频或音频文件路径 | （必填） |
| `--output` | `-o` | 输出文件路径 | `<input>.<format>` |
| `--format` | `-f` | 输出格式 (json, srt, vtt, txt) | `json` |
| `--keep-audio` | `-k` | 保留中间生成的 MP3 文件 | `false` |
| `--model` | | 使用的 Gemini 模型 | `gemini-2.5-flash` |
| `--timeout` | | API 超时时间（秒） | `600` |
| `--max-retries` | | API 调用最大重试次数 | `3` |
| `--force-file-api` | | 强制使用 File API（即使是小文件） | `false` |
| `--keep-remote-file` | | 保留上传到服务器的文件 | `false` |
| `--verbose` | `-v` | 详细程度 (-v, -vv, -vvv) | warn |
| `--quiet` | `-q` | 安静模式（无进度输出） | `false` |
| `--help` | `-h` | 显示帮助信息 | |
| `--version` | `-V` | 显示版本 | |

### 批量处理 (`batch_convert`)

递归处理一个或多个文件夹中的所有媒体文件。

```bash
# 处理文件夹中的所有媒体文件
batch_convert /path/to/folder

# 处理多个文件夹
batch_convert folder1 folder2 folder3

# 输出为 SRT 格式
batch_convert /path/to/folder -f srt

# 控制并行任务数（默认：2）
batch_convert /path/to/folder -j 4

# 调整任务间延迟以避免速率限制（默认：5 秒）
batch_convert /path/to/folder -d 10

# 保守设置，用于严格的速率限制
batch_convert /path/to/folder -j 1 -d 10

# 启用详细日志
batch_convert /path/to/folder -v
```

#### 命令行选项

| 选项 | 简写 | 描述 | 默认值 |
|------|------|------|--------|
| `FOLDERS` | | 要处理的文件夹路径（递归） | （必填） |
| `--format` | `-f` | 输出格式 (json, srt, vtt, txt) | `json` |
| `--jobs` | `-j` | 并行任务数 | `2` |
| `--delay` | `-d` | 启动任务之间的延迟（秒） | `5` |
| `--keep-audio` | `-k` | 保留中间生成的 MP3 文件 | `false` |
| `--model` | | 使用的 Gemini 模型 | `gemini-2.5-flash` |
| `--timeout` | | API 超时时间（秒） | `600` |
| `--max-retries` | | API 调用最大重试次数 | `3` |
| `--force-file-api` | | 强制使用 File API（即使是小文件） | `false` |
| `--keep-remote-file` | | 保留上传到服务器的文件 | `false` |
| `--verbose` | `-v` | 详细程度 (-v, -vv, -vvv) | warn |
| `--quiet` | `-q` | 安静模式（无进度输出） | `false` |
| `--help` | `-h` | 显示帮助信息 | |
| `--version` | `-V` | 显示版本 | |

### 图像生成 (`imagen`)

使用 Gemini 图像模型从文本提示生成图像。

```bash
# 基本用法 - 从提示生成图像
imagen "山脉上的日落"

# 使用 Gemini 3 Pro 模型（支持尺寸/宽高比选项）
imagen -m 3pro "未来城市景观"

# 高分辨率自定义宽高比（仅 Gemini 3 Pro）
imagen -m 3pro --size 2K --aspect 16:9 "宽幅全景风景"

# 指定输出文件
imagen "一只猫" -o cat.png

# 从 YAML 文件生成
imagen --yaml prompts.yaml

# 从 YAML 生成特定提示
imagen --yaml prompts.yaml --name memory-safety

# 4 个并行任务
imagen --yaml prompts.yaml -j 4

# 安静模式
imagen --yaml prompts.yaml -q
```

#### YAML 格式

```yaml
prompts:
  - name: sunset
    prompt: 美丽的山脉日落
  - name: cityscape
    prompt: 夜晚的未来城市景观
    model: 3pro        # 可选：覆盖模型
    size: 2K           # 可选：仅 Gemini 3 Pro
    aspect: 16:9       # 可选：仅 Gemini 3 Pro
    output: city.png   # 可选：自定义文件名
```

#### 命令行选项

| 选项 | 简写 | 描述 | 默认值 |
|------|------|------|--------|
| `PROMPT` | | 图像生成的文本提示 | |
| `--yaml` | `-y` | 包含提示的 YAML 文件 | |
| `--name` | `-n` | 从 YAML 生成特定提示 | |
| `--output` | `-o` | 输出文件/目录 | `./output` |
| `--model` | `-m` | 模型：`2.5-flash`、`3pro` | `2.5-flash` |
| `--size` | `-s` | 图像尺寸：`1K`、`2K`、`4K`（仅 3pro） | `1K` |
| `--aspect` | `-a` | 宽高比（仅 3pro） | `1:1` |
| `--jobs` | `-j` | YAML 批量的并行任务数 | `2` |
| `--timeout` | `-t` | API 超时时间（秒） | `120` |
| `--max-retries` | | 最大重试次数 | `3` |
| `--verbose` | `-v` | 详细程度（-v、-vv、-vvv） | warn |
| `--quiet` | `-q` | 安静模式（无进度输出） | `false` |
| `--help` | `-h` | 显示帮助信息 | |
| `--version` | `-V` | 显示版本 | |

#### 支持的模型

| 模型 | 标志 | 功能 |
|------|------|------|
| Gemini 2.5 Flash Image | `-m 2.5-flash` | 快速生成 |
| Gemini 3 Pro Image | `-m 3pro` | 尺寸（1K/2K/4K）、宽高比选项 |

#### 宽高比（Gemini 3 Pro）

- `1:1` - 正方形（默认）
- `16:9` - 宽屏/横向
- `9:16` - 竖屏/纵向
- `4:3` - 标准
- `3:4` - 肖像

### 图像编辑 (`imagen_edit`)

使用 Gemini 3 Pro 模型通过文本提示编辑和转换图像。

```bash
# 单张图像编辑
imagen_edit -i photo.jpg "把它变成水彩画风格"

# 多张输入图像（例如：将多张人脸合成为合照）
imagen_edit -i face1.png -i face2.png -i face3.png "这些人的办公室合照"

# 带尺寸和宽高比选项
imagen_edit -i img1.jpg -i img2.jpg --size 2K --aspect 16:9 "合成全景图"

# 指定输出文件
imagen_edit -i portrait.png -o edited.png "添加日落背景"

# YAML 批量模式
imagen_edit --yaml edits.yaml

# 从 YAML 处理特定条目
imagen_edit --yaml edits.yaml --name group-photo

# 4 个并行任务
imagen_edit --yaml edits.yaml -j 4

# 自定义输出目录
imagen_edit --yaml edits.yaml -o ./results
```

#### YAML 格式

```yaml
edits:
  - name: group-photo
    prompt: 这些人的办公室合照，他们在做鬼脸
    images:
      - face1.png
      - face2.png
      - face3.png
    output: group.png   # 可选：自定义文件名
  - name: watercolor
    prompt: 把它变成水彩画风格
    images:
      - photo.jpg
  - name: panorama
    prompt: 合成宽幅全景图
    images:
      - img1.jpg
      - img2.jpg
    size: 2K            # 可选
    aspect: 16:9        # 可选
```

#### 命令行选项

| 选项 | 简写 | 描述 | 默认值 |
|------|------|------|--------|
| `--input` | `-i` | 输入图像文件，可指定多个 | |
| `PROMPT` | | 描述编辑内容的文本提示 | |
| `--yaml` | `-y` | 包含编辑任务的 YAML 文件 | |
| `--name` | `-n` | 从 YAML 处理特定条目 | |
| `--output` | `-o` | 输出文件/目录 | `./output` |
| `--size` | `-s` | 图像尺寸：`1K`、`2K`、`4K` | `1K` |
| `--aspect` | `-a` | 宽高比 | `1:1` |
| `--jobs` | `-j` | YAML 批量的并行任务数 | `2` |
| `--timeout` | `-t` | API 超时时间（秒） | `120` |
| `--max-retries` | | 最大重试次数 | `3` |
| `--verbose` | `-v` | 详细程度（-v、-vv、-vvv） | warn |
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
mp4、mkv、avi、mov、webm、flv、wmv、m4v

### 输入音频格式
mp3、wav、ogg、flac、m4a、aac、wma

## 智能功能

- **跳过已有转录**：`convert` 和 `batch_convert` 都会自动跳过已有转录输出文件的媒体文件
- **输入验证**：验证输入文件是支持的媒体格式，验证输入路径是目录（对于 batch_convert）
- **大文件支持**：超过 20MB 的文件自动使用 Gemini File API 进行可恢复上传（最大支持 2GB）

## 错误处理

工具包含健壮的错误处理：

- **重试逻辑**：网络错误和服务器错误（5xx）时自动使用指数退避重试
- **智能速率限制**：检测 429 响应并使用更长的退避时间（30 秒、60 秒、90 秒）以避免配额耗尽
- **批量速率控制**：使用 `--delay` 和 `--jobs` 选项控制批量模式下的 API 请求速率
- **超时配置**：可为长音频文件配置超时时间（默认：10 分钟）

## 许可证

MIT
