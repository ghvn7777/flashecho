use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tracing::{Level, debug, info, warn};
use tracing_subscriber::FmtSubscriber;
use walkdir::WalkDir;

use transcript_tool::{
    FileApiClient, GeminiClient, GeminiClientConfig, MAX_INLINE_FILE_SIZE, TranscriptResponse,
};

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum OutputFormat {
    #[default]
    Json,
    Srt,
    Vtt,
    Txt,
}

#[derive(Parser, Debug)]
#[command(name = "batch_convert")]
#[command(version)]
#[command(about = "Batch convert video/audio files to transcripts using Gemini API")]
struct Args {
    /// Folder paths to process (recursive)
    #[arg(required = true)]
    folders: Vec<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "json")]
    format: OutputFormat,

    /// Number of parallel jobs
    #[arg(short, long, default_value = "2")]
    jobs: usize,

    /// Delay in seconds between starting new tasks (helps avoid rate limiting)
    #[arg(short, long, default_value = "5")]
    delay: u64,

    /// Keep the intermediate MP3 files
    #[arg(short, long, default_value = "false")]
    keep_audio: bool,

    /// Gemini model to use
    #[arg(long, default_value = "gemini-2.5-flash")]
    model: String,

    /// API timeout in seconds
    #[arg(long, default_value = "600")]
    timeout: u64,

    /// Max retry attempts for API calls
    #[arg(long, default_value = "3")]
    max_retries: u32,

    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Quiet mode (no progress output)
    #[arg(short, long)]
    quiet: bool,

    /// Force use of File API even for small files
    #[arg(long)]
    force_file_api: bool,

    /// Keep uploaded file on server (don't delete after transcription)
    #[arg(long)]
    keep_remote_file: bool,
}

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v"];
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "flac", "m4a", "aac", "wma"];

fn get_api_key() -> Result<String> {
    std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_AI_KEY"))
        .context("GEMINI_API_KEY or GOOGLE_AI_KEY environment variable is not set")
}

fn is_media_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext_lower = ext.to_lowercase();
            VIDEO_EXTENSIONS.contains(&ext_lower.as_str())
                || AUDIO_EXTENSIONS.contains(&ext_lower.as_str())
        })
        .unwrap_or(false)
}

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn find_media_files(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for folder in folders {
        if !folder.exists() {
            warn!("Folder does not exist: {:?}", folder);
            continue;
        }
        for entry in WalkDir::new(folder)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() && is_media_file(path) {
                files.push(path.to_path_buf());
            }
        }
    }
    files
}

async fn extract_audio_with_ffmpeg(input: &Path, output: &Path) -> Result<()> {
    debug!("Extracting audio from {:?} to {:?}", input, output);

    let output_result = Command::new("ffmpeg")
        .args([
            "-i",
            input.to_str().context("Invalid input path")?,
            "-vn",
            "-acodec",
            "libmp3lame",
            "-q:a",
            "2",
            "-y",
            output.to_str().context("Invalid output path")?,
        ])
        .output()
        .await
        .context("Failed to execute ffmpeg. Is ffmpeg installed?")?;

    if !output_result.status.success() {
        let stderr = String::from_utf8_lossy(&output_result.stderr);
        anyhow::bail!("ffmpeg failed: {}", stderr);
    }

    Ok(())
}

fn format_timestamp_srt(timestamp: &str) -> String {
    let parts: Vec<&str> = timestamp.split(':').collect();
    if parts.len() == 2 {
        format!("00:{}:{},000", parts[0], parts[1])
    } else {
        format!("00:{},000", timestamp)
    }
}

fn format_timestamp_vtt(timestamp: &str) -> String {
    let parts: Vec<&str> = timestamp.split(':').collect();
    if parts.len() == 2 {
        format!("00:{}:{}.000", parts[0], parts[1])
    } else {
        format!("00:{}.000", timestamp)
    }
}

fn transcript_to_srt(transcript: &TranscriptResponse) -> String {
    let mut output = String::new();
    for (i, segment) in transcript.segments.iter().enumerate() {
        let start = format_timestamp_srt(&segment.timestamp);
        let end = if i + 1 < transcript.segments.len() {
            format_timestamp_srt(&transcript.segments[i + 1].timestamp)
        } else {
            let parts: Vec<&str> = segment.timestamp.split(':').collect();
            if parts.len() == 2 {
                let mins: u32 = parts[0].parse().unwrap_or(0);
                let secs: u32 = parts[1].parse().unwrap_or(0) + 5;
                let new_mins = mins + secs / 60;
                let new_secs = secs % 60;
                format!("00:{:02}:{:02},000", new_mins, new_secs)
            } else {
                "00:00:05,000".to_string()
            }
        };
        output.push_str(&format!("{}\n", i + 1));
        output.push_str(&format!("{} --> {}\n", start, end));
        output.push_str(&format!("[{}] {}\n\n", segment.speaker, segment.content));
    }
    output
}

fn transcript_to_vtt(transcript: &TranscriptResponse) -> String {
    let mut output = String::from("WEBVTT\n\n");
    for (i, segment) in transcript.segments.iter().enumerate() {
        let start = format_timestamp_vtt(&segment.timestamp);
        let end = if i + 1 < transcript.segments.len() {
            format_timestamp_vtt(&transcript.segments[i + 1].timestamp)
        } else {
            let parts: Vec<&str> = segment.timestamp.split(':').collect();
            if parts.len() == 2 {
                let mins: u32 = parts[0].parse().unwrap_or(0);
                let secs: u32 = parts[1].parse().unwrap_or(0) + 5;
                let new_mins = mins + secs / 60;
                let new_secs = secs % 60;
                format!("00:{:02}:{:02}.000", new_mins, new_secs)
            } else {
                "00:00:05.000".to_string()
            }
        };
        output.push_str(&format!("{} --> {}\n", start, end));
        output.push_str(&format!("<v {}>{}\n\n", segment.speaker, segment.content));
    }
    output
}

fn transcript_to_txt(transcript: &TranscriptResponse) -> String {
    let mut output = String::new();
    output.push_str(&format!("Summary:\n{}\n\n", transcript.summary));
    output.push_str("---\n\n");
    for segment in &transcript.segments {
        output.push_str(&format!(
            "[{}] {} ({})\n{}\n",
            segment.timestamp, segment.speaker, segment.emotion, segment.content
        ));
        if let Some(ref translation) = segment.translation
            && !translation.is_empty()
        {
            output.push_str(&format!("  Translation: {}\n", translation));
        }
        output.push('\n');
    }
    output
}

fn format_output(transcript: &TranscriptResponse, format: OutputFormat) -> Result<String> {
    match format {
        OutputFormat::Json => {
            serde_json::to_string_pretty(transcript).context("Failed to serialize to JSON")
        }
        OutputFormat::Srt => Ok(transcript_to_srt(transcript)),
        OutputFormat::Vtt => Ok(transcript_to_vtt(transcript)),
        OutputFormat::Txt => Ok(transcript_to_txt(transcript)),
    }
}

fn get_output_extension(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Json => "json",
        OutputFormat::Srt => "srt",
        OutputFormat::Vtt => "vtt",
        OutputFormat::Txt => "txt",
    }
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => Level::WARN,
        1 => Level::INFO,
        2 => Level::DEBUG,
        _ => Level::TRACE,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .finish();

    tracing::subscriber::set_global_default(subscriber).ok();
}

#[derive(Debug)]
struct ProcessResult {
    path: PathBuf,
    success: bool,
    skipped: bool,
    error: Option<String>,
    segments: usize,
}

#[allow(clippy::too_many_arguments)]
async fn process_file(
    input: PathBuf,
    api_key: String,
    config: GeminiClientConfig,
    format: OutputFormat,
    keep_audio: bool,
    force_file_api: bool,
    keep_remote_file: bool,
    overall_pb: ProgressBar,
) -> ProcessResult {
    let file_name = input
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Check if output file already exists
    let output_path = {
        let mut p = input.clone();
        p.set_extension(get_output_extension(format));
        p
    };

    if output_path.exists() {
        overall_pb.println(format!(
            "  Skipped: {} (transcript already exists)",
            file_name
        ));
        return ProcessResult {
            path: input,
            success: true,
            skipped: true,
            error: None,
            segments: 0,
        };
    }

    overall_pb.println(format!("  Starting: {}", file_name));

    let result = process_file_inner(
        &input,
        &api_key,
        &config,
        format,
        keep_audio,
        force_file_api,
        keep_remote_file,
    )
    .await;

    match result {
        Ok(segments) => {
            overall_pb.println(format!("  Done: {} ({} segments)", file_name, segments));
            ProcessResult {
                path: input,
                success: true,
                skipped: false,
                error: None,
                segments,
            }
        }
        Err(e) => {
            overall_pb.println(format!("  Failed: {}", file_name));
            ProcessResult {
                path: input,
                success: false,
                skipped: false,
                error: Some(e.to_string()),
                segments: 0,
            }
        }
    }
}

async fn process_file_inner(
    input: &Path,
    api_key: &str,
    config: &GeminiClientConfig,
    format: OutputFormat,
    keep_audio: bool,
    force_file_api: bool,
    keep_remote_file: bool,
) -> Result<usize> {
    let (audio_path, should_cleanup) = if is_audio_file(input) {
        (input.to_path_buf(), false)
    } else {
        let mp3_path = input.with_extension("mp3");
        extract_audio_with_ffmpeg(input, &mp3_path).await?;
        (mp3_path, !keep_audio)
    };

    let audio_data = fs::read(&audio_path)
        .await
        .context("Failed to read audio file")?;

    let file_size = audio_data.len() as u64;
    let mime_type = GeminiClient::get_mime_type(&audio_path);

    let client = GeminiClient::with_config(api_key.to_string(), config.clone())
        .map_err(|e| anyhow::anyhow!("Failed to create Gemini client: {}", e))?;

    let use_file_api = force_file_api || file_size > MAX_INLINE_FILE_SIZE;

    let (transcript, uploaded_file_name) = if use_file_api {
        let file_api = FileApiClient::new(client.http_client().clone(), api_key.to_string());

        let display_name = audio_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio");

        let file_info = file_api
            .upload_file(&audio_data, mime_type, display_name)
            .await
            .map_err(|e| anyhow::anyhow!("File upload failed: {}", e))?;

        let transcript = client
            .transcribe_file_uri(&file_info.uri, mime_type)
            .await
            .map_err(|e| anyhow::anyhow!("Transcription failed: {}", e))?;

        (transcript, Some((file_api, file_info.name)))
    } else {
        let transcript = client
            .transcribe_audio(&audio_data, mime_type)
            .await
            .map_err(|e| anyhow::anyhow!("Transcription failed: {}", e))?;
        (transcript, None)
    };

    let segment_count = transcript.segments.len();

    let output_path = {
        let mut p = input.to_path_buf();
        p.set_extension(get_output_extension(format));
        p
    };

    let formatted_output = format_output(&transcript, format)?;
    fs::write(&output_path, &formatted_output)
        .await
        .context("Failed to write output file")?;

    info!("Transcript saved to: {:?}", output_path);

    // Cleanup remote file if uploaded
    if let Some((file_api, file_name)) = uploaded_file_name
        && !keep_remote_file
        && let Err(e) = file_api.delete_file(&file_name).await
    {
        warn!("Failed to delete remote file {}: {}", file_name, e);
    }

    // Cleanup temp audio file
    if should_cleanup {
        fs::remove_file(&audio_path).await.ok();
    }

    Ok(segment_count)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    init_logging(args.verbose);

    let api_key = get_api_key()?;

    // Validate input folders
    for folder in &args.folders {
        if !folder.exists() {
            anyhow::bail!("Folder does not exist: {:?}", folder);
        }
        if !folder.is_dir() {
            anyhow::bail!(
                "Path is not a directory: {:?}\nUse 'convert' command for single files.",
                folder
            );
        }
    }

    // Find all media files
    let files = find_media_files(&args.folders);

    if files.is_empty() {
        println!("No video or audio files found in the specified folders.");
        return Ok(());
    }

    let files_count = files.len();
    println!("Found {} files to process", files_count);

    let config = GeminiClientConfig {
        timeout_secs: args.timeout,
        max_retries: args.max_retries,
        model: args.model.clone(),
    };

    let semaphore = Arc::new(Semaphore::new(args.jobs));

    let overall_pb = ProgressBar::new(files_count as u64);
    overall_pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:40.cyan/blue}] {pos}/{len} files ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut handles = Vec::new();
    let delay = Duration::from_secs(args.delay);

    for (i, file) in files.into_iter().enumerate() {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let api_key = api_key.clone();
        let config = config.clone();
        let format = args.format;
        let keep_audio = args.keep_audio;
        let force_file_api = args.force_file_api;
        let keep_remote_file = args.keep_remote_file;
        let overall_pb = overall_pb.clone();

        let handle = tokio::spawn(async move {
            let result = process_file(
                file,
                api_key,
                config,
                format,
                keep_audio,
                force_file_api,
                keep_remote_file,
                overall_pb.clone(),
            )
            .await;
            overall_pb.inc(1);
            drop(permit);
            result
        });

        handles.push(handle);

        // Add delay between starting tasks to avoid rate limiting
        // Skip delay after the last file
        if i < files_count - 1 && delay.as_secs() > 0 {
            tokio::time::sleep(delay).await;
        }
    }

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }

    overall_pb.finish_and_clear();

    // Print summary
    let processed: Vec<_> = results.iter().filter(|r| r.success && !r.skipped).collect();
    let skipped: Vec<_> = results.iter().filter(|r| r.skipped).collect();
    let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();
    let total_segments: usize = processed.iter().map(|r| r.segments).sum();

    println!("\n--- Batch Processing Complete ---");
    println!(
        "Processed: {} files ({} total segments)",
        processed.len(),
        total_segments
    );
    if !skipped.is_empty() {
        println!(
            "Skipped: {} files (already have transcripts)",
            skipped.len()
        );
    }

    if !failed.is_empty() {
        println!("\nFailed: {} files", failed.len());
        for result in &failed {
            println!(
                "  - {:?}: {}",
                result.path,
                result.error.as_deref().unwrap_or("Unknown error")
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_media_file() {
        assert!(is_media_file(Path::new("test.mp4")));
        assert!(is_media_file(Path::new("test.MP4")));
        assert!(is_media_file(Path::new("test.mkv")));
        assert!(is_media_file(Path::new("test.mp3")));
        assert!(is_media_file(Path::new("test.wav")));
        assert!(!is_media_file(Path::new("test.txt")));
        assert!(!is_media_file(Path::new("test.json")));
    }

    #[test]
    fn test_is_audio_file() {
        assert!(is_audio_file(Path::new("test.mp3")));
        assert!(is_audio_file(Path::new("test.wav")));
        assert!(!is_audio_file(Path::new("test.mp4")));
        assert!(!is_audio_file(Path::new("test.mkv")));
    }

    #[test]
    fn test_get_output_extension() {
        assert_eq!(get_output_extension(OutputFormat::Json), "json");
        assert_eq!(get_output_extension(OutputFormat::Srt), "srt");
        assert_eq!(get_output_extension(OutputFormat::Vtt), "vtt");
        assert_eq!(get_output_extension(OutputFormat::Txt), "txt");
    }
}
