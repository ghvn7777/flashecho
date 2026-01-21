mod file_api;
mod gemini_api;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use file_api::FileApiClient;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;
use tokio::process::Command;
use tracing::{Level, debug, info, warn};
use tracing_subscriber::FmtSubscriber;

use gemini_api::{GeminiClient, GeminiClientConfig, MAX_INLINE_FILE_SIZE, TranscriptResponse};

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum OutputFormat {
    #[default]
    Json,
    Srt,
    Vtt,
    Txt,
}

#[derive(Parser, Debug)]
#[command(name = "convert")]
#[command(version)]
#[command(about = "Extract audio from video and transcribe using Gemini API")]
struct Args {
    /// Input video or audio file path
    #[arg(short, long)]
    input: PathBuf,

    /// Output file path (defaults to <input>.<format>)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "json")]
    format: OutputFormat,

    /// Keep the intermediate MP3 file
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

fn get_api_key() -> Result<String> {
    std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_AI_KEY"))
        .context("GEMINI_API_KEY or GOOGLE_AI_KEY environment variable is not set")
}

fn is_audio_file(path: &Path) -> bool {
    let audio_extensions = ["mp3", "wav", "ogg", "flac", "m4a", "aac", "wma", "webm"];
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| audio_extensions.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

async fn extract_audio_with_ffmpeg(input: &Path, output: &Path, quiet: bool) -> Result<()> {
    info!("Extracting audio from {:?} to {:?}", input, output);

    let pb = if !quiet {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        pb.set_message("Extracting audio with ffmpeg...");
        pb.enable_steady_tick(Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

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

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    if !output_result.status.success() {
        let stderr = String::from_utf8_lossy(&output_result.stderr);
        anyhow::bail!("ffmpeg failed: {}", stderr);
    }

    info!("Audio extraction complete");
    Ok(())
}

fn format_timestamp_srt(timestamp: &str) -> String {
    // Convert MM:SS to SRT format 00:MM:SS,000
    let parts: Vec<&str> = timestamp.split(':').collect();
    if parts.len() == 2 {
        format!("00:{}:{},000", parts[0], parts[1])
    } else {
        format!("00:{},000", timestamp)
    }
}

fn format_timestamp_vtt(timestamp: &str) -> String {
    // Convert MM:SS to VTT format 00:MM:SS.000
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
        // Estimate end time as 5 seconds after start (or use next segment's start)
        let end = if i + 1 < transcript.segments.len() {
            format_timestamp_srt(&transcript.segments[i + 1].timestamp)
        } else {
            // Add 5 seconds to last timestamp
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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    init_logging(args.verbose);

    let api_key = get_api_key()?;

    if !args.input.exists() {
        anyhow::bail!("Input file does not exist: {:?}", args.input);
    }

    let (audio_path, should_cleanup) = if is_audio_file(&args.input) {
        info!("Input is already an audio file, skipping ffmpeg extraction");
        if !args.quiet {
            println!("Input is already an audio file, skipping extraction.");
        }
        (args.input.clone(), false)
    } else {
        let mp3_path = args.input.with_extension("mp3");
        extract_audio_with_ffmpeg(&args.input, &mp3_path, args.quiet).await?;
        if !args.quiet {
            println!("Audio extracted successfully.");
        }
        (mp3_path, !args.keep_audio)
    };

    debug!("Reading audio file: {:?}", audio_path);
    let audio_data = fs::read(&audio_path)
        .await
        .context("Failed to read audio file")?;

    let file_size = audio_data.len() as u64;
    debug!("Audio file size: {} bytes", file_size);

    // Get correct MIME type
    let mime_type = GeminiClient::get_mime_type(&audio_path);
    debug!("Detected MIME type: {}", mime_type);

    let config = GeminiClientConfig {
        timeout_secs: args.timeout,
        max_retries: args.max_retries,
        model: args.model.clone(),
    };

    let client = GeminiClient::with_config(api_key.clone(), config)
        .map_err(|e| anyhow::anyhow!("Failed to create Gemini client: {}", e))?;

    // Determine if we need to use the File API
    let use_file_api = args.force_file_api || file_size > MAX_INLINE_FILE_SIZE;

    let (transcript, uploaded_file_name) = if use_file_api {
        // Use File API for large files
        let size_mb = file_size as f64 / (1024.0 * 1024.0);
        if !args.quiet {
            if args.force_file_api && file_size <= MAX_INLINE_FILE_SIZE {
                println!("Using File API (forced) for {:.1}MB file...", size_mb);
            } else {
                println!(
                    "File size {:.1}MB exceeds 20MB limit, using File API for upload...",
                    size_mb
                );
            }
        }
        info!(
            "Using File API for file size {} bytes ({:.1}MB)",
            file_size, size_mb
        );

        let file_api = FileApiClient::new(client.http_client().clone(), api_key);

        // Upload progress
        let upload_pb = if !args.quiet {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg}")
                    .unwrap(),
            );
            pb.set_message("Uploading audio to Gemini File API...");
            pb.enable_steady_tick(Duration::from_millis(100));
            Some(pb)
        } else {
            None
        };

        let display_name = audio_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio");

        let file_info = file_api
            .upload_file(&audio_data, mime_type, display_name)
            .await
            .map_err(|e| anyhow::anyhow!("File upload failed: {}", e))?;

        if let Some(pb) = upload_pb {
            pb.finish_with_message("Upload complete!");
        }
        if !args.quiet {
            println!("File uploaded successfully.");
        }
        info!("File uploaded: {} -> {}", file_info.name, file_info.uri);

        // Transcription progress
        let transcribe_pb = if !args.quiet {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .unwrap(),
            );
            pb.set_message("Transcribing audio with Gemini API...");
            pb.enable_steady_tick(Duration::from_millis(100));
            Some(pb)
        } else {
            None
        };

        let transcript = client
            .transcribe_file_uri(&file_info.uri, mime_type)
            .await
            .map_err(|e| anyhow::anyhow!("Transcription failed: {}", e))?;

        if let Some(pb) = transcribe_pb {
            pb.finish_with_message("Transcription complete!");
        }

        (transcript, Some((file_api, file_info.name)))
    } else {
        // Use inline data for small files
        let pb = if !args.quiet {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .unwrap(),
            );
            pb.set_message("Transcribing audio with Gemini API...");
            pb.enable_steady_tick(Duration::from_millis(100));
            Some(pb)
        } else {
            None
        };

        let transcript = client
            .transcribe_audio(&audio_data, mime_type)
            .await
            .map_err(|e| anyhow::anyhow!("Transcription failed: {}", e))?;

        if let Some(pb) = pb {
            pb.finish_with_message("Transcription complete!");
        }

        (transcript, None)
    };

    let output_path = args.output.unwrap_or_else(|| {
        let mut p = args.input.clone();
        p.set_extension(get_output_extension(args.format));
        p
    });

    let formatted_output = format_output(&transcript, args.format)?;

    fs::write(&output_path, &formatted_output)
        .await
        .context("Failed to write output file")?;

    if !args.quiet {
        println!("Transcript saved to: {:?}", output_path);
    }
    info!("Transcript saved to: {:?}", output_path);

    // Cleanup remote file if uploaded (unless --keep-remote-file was specified)
    if let Some((file_api, file_name)) = uploaded_file_name {
        if args.keep_remote_file {
            if !args.quiet {
                println!("Keeping remote file: {}", file_name);
            }
            info!("Keeping remote file: {}", file_name);
        } else {
            debug!("Cleaning up remote file: {}", file_name);
            match file_api.delete_file(&file_name).await {
                Ok(()) => {
                    if !args.quiet {
                        println!("Remote file deleted.");
                    }
                    info!("Remote file deleted: {}", file_name);
                }
                Err(e) => {
                    warn!("Failed to delete remote file {}: {}", file_name, e);
                    if !args.quiet {
                        println!("Warning: Failed to delete remote file: {}", e);
                    }
                }
            }
        }
    }

    if should_cleanup {
        debug!("Cleaning up temporary audio file: {:?}", audio_path);
        fs::remove_file(&audio_path).await.ok();
    }

    if !args.quiet {
        println!("\nSummary: {}", transcript.summary);
        println!("Total segments: {}", transcript.segments.len());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gemini_api::TranscriptSegment;

    fn create_test_transcript() -> TranscriptResponse {
        TranscriptResponse {
            summary: "Test summary".to_string(),
            segments: vec![
                TranscriptSegment {
                    speaker: "Speaker 1".to_string(),
                    timestamp: "00:05".to_string(),
                    content: "Hello world".to_string(),
                    language: "English".to_string(),
                    language_code: "en".to_string(),
                    translation: None,
                    emotion: "neutral".to_string(),
                },
                TranscriptSegment {
                    speaker: "Speaker 2".to_string(),
                    timestamp: "00:10".to_string(),
                    content: "Hi there".to_string(),
                    language: "English".to_string(),
                    language_code: "en".to_string(),
                    translation: None,
                    emotion: "happy".to_string(),
                },
            ],
        }
    }

    #[test]
    fn test_is_audio_file() {
        assert!(is_audio_file(Path::new("test.mp3")));
        assert!(is_audio_file(Path::new("test.MP3")));
        assert!(is_audio_file(Path::new("test.wav")));
        assert!(is_audio_file(Path::new("test.WAV")));
        assert!(is_audio_file(Path::new("test.flac")));
        assert!(!is_audio_file(Path::new("test.mp4")));
        assert!(!is_audio_file(Path::new("test.txt")));
    }

    #[test]
    fn test_format_timestamp_srt() {
        assert_eq!(format_timestamp_srt("05:30"), "00:05:30,000");
        assert_eq!(format_timestamp_srt("00:05"), "00:00:05,000");
    }

    #[test]
    fn test_format_timestamp_vtt() {
        assert_eq!(format_timestamp_vtt("05:30"), "00:05:30.000");
        assert_eq!(format_timestamp_vtt("00:05"), "00:00:05.000");
    }

    #[test]
    fn test_transcript_to_srt() {
        let transcript = create_test_transcript();
        let srt = transcript_to_srt(&transcript);

        assert!(srt.contains("1\n"));
        assert!(srt.contains("00:00:05,000 --> 00:00:10,000"));
        assert!(srt.contains("[Speaker 1] Hello world"));
    }

    #[test]
    fn test_transcript_to_vtt() {
        let transcript = create_test_transcript();
        let vtt = transcript_to_vtt(&transcript);

        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("00:00:05.000 --> 00:00:10.000"));
        assert!(vtt.contains("<v Speaker 1>Hello world"));
    }

    #[test]
    fn test_transcript_to_txt() {
        let transcript = create_test_transcript();
        let txt = transcript_to_txt(&transcript);

        assert!(txt.contains("Summary:"));
        assert!(txt.contains("Test summary"));
        assert!(txt.contains("[00:05] Speaker 1 (neutral)"));
        assert!(txt.contains("Hello world"));
    }

    #[test]
    fn test_get_output_extension() {
        assert_eq!(get_output_extension(OutputFormat::Json), "json");
        assert_eq!(get_output_extension(OutputFormat::Srt), "srt");
        assert_eq!(get_output_extension(OutputFormat::Vtt), "vtt");
        assert_eq!(get_output_extension(OutputFormat::Txt), "txt");
    }
}
