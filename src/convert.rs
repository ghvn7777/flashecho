mod gemini_api;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs;

use gemini_api::GeminiClient;

#[derive(Parser, Debug)]
#[command(name = "convert")]
#[command(about = "Extract audio from video and transcribe using Gemini API")]
struct Args {
    /// Input video or audio file path
    #[arg(short, long)]
    input: PathBuf,

    /// Output JSON file path (defaults to <input>.json)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Keep the intermediate MP3 file
    #[arg(short, long, default_value = "false")]
    keep_audio: bool,
}

fn get_api_key() -> Result<String> {
    std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_AI_KEY"))
        .context("GEMINI_API_KEY or GOOGLE_AI_KEY environment variable is not set")
}

fn is_audio_file(path: &Path) -> bool {
    let audio_extensions = ["mp3", "wav", "ogg", "flac", "m4a", "aac", "wma"];
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| audio_extensions.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn extract_audio_with_ffmpeg(input: &Path, output: &Path) -> Result<()> {
    println!("Extracting audio from {:?} to {:?}", input, output);

    let status = Command::new("ffmpeg")
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
        .status()
        .context("Failed to execute ffmpeg. Is ffmpeg installed?")?;

    if !status.success() {
        anyhow::bail!("ffmpeg failed with exit code: {:?}", status.code());
    }

    println!("Audio extraction complete.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let api_key = get_api_key()?;

    if !args.input.exists() {
        anyhow::bail!("Input file does not exist: {:?}", args.input);
    }

    let (audio_path, should_cleanup) = if is_audio_file(&args.input) {
        println!("Input is already an audio file, skipping ffmpeg extraction.");
        (args.input.clone(), false)
    } else {
        let mp3_path = args.input.with_extension("mp3");
        extract_audio_with_ffmpeg(&args.input, &mp3_path)?;
        (mp3_path, !args.keep_audio)
    };

    println!("Reading audio file: {:?}", audio_path);
    let audio_data = fs::read(&audio_path)
        .await
        .context("Failed to read audio file")?;

    println!("Sending audio to Gemini API for transcription...");
    let client = GeminiClient::new(api_key)?;
    let transcript = client.transcribe_audio(&audio_data, "audio/mpeg").await?;

    let output_path = args.output.unwrap_or_else(|| {
        let mut p = args.input.clone();
        p.set_extension("json");
        p
    });

    let json_output = serde_json::to_string_pretty(&transcript)
        .context("Failed to serialize transcript to JSON")?;

    fs::write(&output_path, &json_output)
        .await
        .context("Failed to write output JSON file")?;

    println!("Transcript saved to: {:?}", output_path);

    if should_cleanup {
        println!("Cleaning up temporary audio file: {:?}", audio_path);
        fs::remove_file(&audio_path).await.ok();
    }

    println!("\nSummary: {}", transcript.summary);
    println!("Total segments: {}", transcript.segments.len());

    Ok(())
}
