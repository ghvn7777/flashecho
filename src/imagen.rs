use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::sync::Semaphore;
use tracing::{Level, debug};
use tracing_subscriber::FmtSubscriber;

use transcript_tool::imagen_api::{AspectRatio, ImageGenConfig, ImageModel, ImageSize};
use transcript_tool::{GeneratedImage, ImagenClient, ImagenClientConfig};

#[derive(Parser, Debug)]
#[command(name = "imagen")]
#[command(version)]
#[command(about = "Generate images using Gemini API")]
#[command(after_help = "EXAMPLES:
    imagen \"A sunset over mountains\"
    imagen -m 3pro \"A futuristic cityscape\"
    imagen -m 3pro --size 2K --aspect 16:9 \"Wide panorama\"
    imagen --yaml prompts.yaml
    imagen --yaml prompts.yaml --name memory-safety")]
struct Args {
    /// Text prompt for image generation (positional argument)
    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,

    /// YAML file containing prompts
    #[arg(short = 'y', long)]
    yaml: Option<PathBuf>,

    /// Generate only specific prompt by name from YAML
    #[arg(short = 'n', long)]
    name: Option<String>,

    /// Output file path (for single prompt) or directory (for YAML batch)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Gemini model to use: 2.5-flash (default), 3pro
    #[arg(short = 'm', long, default_value = "2.5-flash")]
    model: String,

    /// Image size (Gemini 3 Pro only): 1K, 2K, 4K
    #[arg(short = 's', long)]
    size: Option<String>,

    /// Aspect ratio (Gemini 3 Pro only): 1:1, 16:9, 9:16, 4:3, 3:4
    #[arg(short = 'a', long)]
    aspect: Option<String>,

    /// API timeout in seconds
    #[arg(short, long, default_value = "120")]
    timeout: u64,

    /// Max retry attempts for API calls
    #[arg(long, default_value = "3")]
    max_retries: u32,

    /// Number of parallel jobs for YAML batch mode
    #[arg(short = 'j', long, default_value = "2")]
    jobs: usize,

    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Quiet mode (no progress output)
    #[arg(short, long)]
    quiet: bool,
}

/// YAML file structure for batch prompts
#[derive(Debug, Deserialize)]
struct PromptsFile {
    prompts: Vec<PromptEntry>,
}

#[derive(Debug, Deserialize)]
struct PromptEntry {
    name: String,
    prompt: String,
    output: Option<String>,
    /// Model override for this prompt
    model: Option<String>,
    /// Image size (Gemini 3 Pro only)
    size: Option<String>,
    /// Aspect ratio (Gemini 3 Pro only)
    aspect: Option<String>,
}

fn get_api_key() -> Result<String> {
    std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_AI_KEY"))
        .context("GEMINI_API_KEY or GOOGLE_AI_KEY environment variable is not set")
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

fn parse_model(model_str: &str) -> Result<ImageModel> {
    ImageModel::from_str(model_str).map_err(|e| anyhow::anyhow!("{}", e))
}

fn parse_size(size_str: &str) -> Result<ImageSize> {
    ImageSize::from_str(size_str).map_err(|e| anyhow::anyhow!("{}", e))
}

fn parse_aspect(aspect_str: &str) -> Result<AspectRatio> {
    AspectRatio::from_str(aspect_str).map_err(|e| anyhow::anyhow!("{}", e))
}

fn build_gen_config(
    size: Option<&String>,
    aspect: Option<&String>,
) -> Result<Option<ImageGenConfig>> {
    if size.is_none() && aspect.is_none() {
        return Ok(None);
    }

    let mut config = ImageGenConfig::new();

    if let Some(s) = size {
        config.size = Some(parse_size(s)?);
    }
    if let Some(a) = aspect {
        config.aspect_ratio = Some(parse_aspect(a)?);
    }

    Ok(Some(config))
}

/// Convert a string to a URL-friendly slug
fn slugify(s: &str) -> String {
    let slug: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    // If slug is empty (e.g., emoji-only names), use "image"
    if slug.is_empty() {
        "image".to_string()
    } else {
        slug
    }
}

/// Generate output filename: slug(name)-hash(name+prompt).ext
fn generate_output_filename(name: &str, prompt: &str, extension: &str) -> String {
    let slug = slugify(name);
    let hash_input = format!("{}{}", name, prompt);
    let hash = blake3::hash(hash_input.as_bytes());
    let hash_prefix = &hash.to_hex()[..6];
    format!("{}-{}.{}", slug, hash_prefix, extension)
}

async fn save_image(image: &GeneratedImage, path: &PathBuf) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)
            .await
            .context("Failed to create output directory")?;
    }

    fs::write(path, &image.data)
        .await
        .context("Failed to write image file")?;

    Ok(())
}

async fn generate_single(
    client: &ImagenClient,
    prompt: &str,
    output_path: PathBuf,
    gen_config: Option<&ImageGenConfig>,
    quiet: bool,
) -> Result<()> {
    let pb = if !quiet {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        pb.set_message("Generating image...");
        pb.enable_steady_tick(Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    let image = client
        .generate_image_with_config(prompt, gen_config)
        .await
        .map_err(|e| anyhow::anyhow!("Image generation failed: {}", e))?;

    if let Some(pb) = pb {
        pb.finish_with_message("Image generated!");
    }

    // Determine final output path with correct extension
    let final_path = if output_path.extension().is_none() {
        output_path.with_extension(image.extension())
    } else {
        output_path
    };

    save_image(&image, &final_path).await?;

    Ok(())
}

struct YamlGenOptions {
    api_key: String,
    yaml_path: PathBuf,
    name_filter: Option<String>,
    output_dir: PathBuf,
    default_model: ImageModel,
    default_size: Option<String>,
    default_aspect: Option<String>,
    timeout: u64,
    max_retries: u32,
    jobs: usize,
    quiet: bool,
}

/// Result of a single image generation task
struct GenResult {
    name: String,
    success: bool,
    error: Option<String>,
}

async fn generate_from_yaml(opts: YamlGenOptions) -> Result<()> {
    let yaml_content = fs::read_to_string(&opts.yaml_path)
        .await
        .context("Failed to read YAML file")?;

    let prompts_file: PromptsFile =
        serde_yaml::from_str(&yaml_content).context("Failed to parse YAML file")?;

    let prompts: Vec<PromptEntry> = if let Some(ref name) = opts.name_filter {
        prompts_file
            .prompts
            .into_iter()
            .filter(|p| &p.name == name)
            .collect()
    } else {
        prompts_file.prompts
    };

    if prompts.is_empty() {
        if let Some(name) = opts.name_filter {
            anyhow::bail!("No prompt found with name: {}", name);
        } else {
            anyhow::bail!("No prompts found in YAML file");
        }
    }

    // Ensure output directory exists
    if !opts.output_dir.exists() {
        fs::create_dir_all(&opts.output_dir)
            .await
            .context("Failed to create output directory")?;
    }

    let total = prompts.len();
    let jobs = opts.jobs.max(1);

    if !opts.quiet {
        println!(
            "Generating {} images with {} parallel jobs...\n",
            total, jobs
        );
    }

    // Create semaphore for concurrency control
    let semaphore = Arc::new(Semaphore::new(jobs));
    let opts = Arc::new(opts);

    // Create multi-progress bar for parallel display
    let multi_progress = Arc::new(MultiProgress::new());

    // Spawn all tasks
    let mut handles = Vec::new();

    for (i, entry) in prompts.into_iter().enumerate() {
        let sem = Arc::clone(&semaphore);
        let opts = Arc::clone(&opts);
        let mp = Arc::clone(&multi_progress);

        let handle = tokio::spawn(async move {
            // Acquire semaphore permit
            let _permit = sem.acquire().await.unwrap();

            // Create progress bar for this task
            let pb = if !opts.quiet {
                let pb = mp.add(ProgressBar::new_spinner());
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .template("{spinner:.green} [{pos}] {msg}")
                        .unwrap(),
                );
                pb.set_position((i + 1) as u64);
                pb.set_message(format!("Generating {}...", entry.name));
                pb.enable_steady_tick(Duration::from_millis(100));
                Some(pb)
            } else {
                None
            };

            // Determine model for this entry
            let model = if let Some(model_str) = &entry.model {
                match parse_model(model_str) {
                    Ok(m) => m,
                    Err(e) => {
                        if let Some(pb) = pb {
                            pb.finish_with_message(format!("{} failed!", entry.name));
                        }
                        return GenResult {
                            name: entry.name.clone(),
                            success: false,
                            error: Some(e.to_string()),
                        };
                    }
                }
            } else {
                opts.default_model
            };

            // Build client
            let config = ImagenClientConfig {
                timeout_secs: opts.timeout,
                max_retries: opts.max_retries,
                model,
            };
            let client = match ImagenClient::with_config(opts.api_key.clone(), config) {
                Ok(c) => c,
                Err(e) => {
                    if let Some(pb) = pb {
                        pb.finish_with_message(format!("{} failed!", entry.name));
                    }
                    return GenResult {
                        name: entry.name.clone(),
                        success: false,
                        error: Some(e.to_string()),
                    };
                }
            };

            // Build gen config
            let size = entry.size.as_ref().or(opts.default_size.as_ref());
            let aspect = entry.aspect.as_ref().or(opts.default_aspect.as_ref());
            let gen_config = match build_gen_config(size, aspect) {
                Ok(c) => c,
                Err(e) => {
                    if let Some(pb) = pb {
                        pb.finish_with_message(format!("{} failed!", entry.name));
                    }
                    return GenResult {
                        name: entry.name.clone(),
                        success: false,
                        error: Some(e.to_string()),
                    };
                }
            };

            // Generate image
            match client
                .generate_image_with_config(&entry.prompt, gen_config.as_ref())
                .await
            {
                Ok(image) => {
                    // Determine output filename
                    let filename = entry.output.clone().unwrap_or_else(|| {
                        generate_output_filename(&entry.name, &entry.prompt, image.extension())
                    });
                    let output_path = opts.output_dir.join(&filename);

                    match save_image(&image, &output_path).await {
                        Ok(()) => {
                            if let Some(pb) = pb {
                                pb.finish_with_message(format!("{} -> {}", entry.name, filename));
                            }
                            GenResult {
                                name: entry.name.clone(),
                                success: true,
                                error: None,
                            }
                        }
                        Err(e) => {
                            if let Some(pb) = pb {
                                pb.finish_with_message(format!("{} failed!", entry.name));
                            }
                            GenResult {
                                name: entry.name.clone(),
                                success: false,
                                error: Some(e.to_string()),
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Some(pb) = pb {
                        pb.finish_with_message(format!("{} failed!", entry.name));
                    }
                    GenResult {
                        name: entry.name.clone(),
                        success: false,
                        error: Some(e.to_string()),
                    }
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    let results: Vec<GenResult> = futures::future::join_all(handles)
        .await
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    // Collect results
    let success_count = results.iter().filter(|r| r.success).count();
    let errors: Vec<_> = results
        .iter()
        .filter(|r| !r.success)
        .map(|r| (r.name.clone(), r.error.clone().unwrap_or_default()))
        .collect();

    // Summary
    if !opts.quiet {
        println!("\n--- Summary ---");
        println!(
            "Total: {}, Success: {}, Failed: {}",
            total,
            success_count,
            errors.len()
        );
        if !errors.is_empty() {
            println!("\nFailed prompts:");
            for (name, error) in &errors {
                println!("  - {}: {}", name, error);
            }
        }
    }

    if success_count == 0 && !errors.is_empty() {
        anyhow::bail!("All image generations failed");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    init_logging(args.verbose);

    // Validate arguments
    if args.prompt.is_none() && args.yaml.is_none() {
        anyhow::bail!(
            "Either a prompt or --yaml file must be provided\n\nUsage:\n  imagen \"your prompt here\"\n  imagen --yaml prompts.yaml"
        );
    }

    if args.prompt.is_some() && args.yaml.is_some() {
        anyhow::bail!("Cannot use both prompt and --yaml at the same time");
    }

    if args.name.is_some() && args.yaml.is_none() {
        anyhow::bail!("--name can only be used with --yaml");
    }

    let api_key = get_api_key()?;
    let model = parse_model(&args.model)?;

    // Warn if size/aspect used with non-3pro model
    if (args.size.is_some() || args.aspect.is_some()) && !model.supports_image_config() {
        eprintln!(
            "Warning: --size and --aspect are only supported with Gemini 3 Pro model (-m 3pro)"
        );
    }

    if let Some(yaml_path) = args.yaml {
        // YAML batch mode
        if !yaml_path.exists() {
            anyhow::bail!("YAML file does not exist: {:?}", yaml_path);
        }

        let output_dir = args.output.unwrap_or_else(|| PathBuf::from("./output"));
        debug!("Output directory: {:?}", output_dir);

        generate_from_yaml(YamlGenOptions {
            api_key,
            yaml_path,
            name_filter: args.name,
            output_dir,
            default_model: model,
            default_size: args.size,
            default_aspect: args.aspect,
            timeout: args.timeout,
            max_retries: args.max_retries,
            jobs: args.jobs,
            quiet: args.quiet,
        })
        .await?;
    } else if let Some(prompt) = args.prompt {
        // Single prompt mode
        let config = ImagenClientConfig {
            timeout_secs: args.timeout,
            max_retries: args.max_retries,
            model,
        };

        let client = ImagenClient::with_config(api_key, config)
            .map_err(|e| anyhow::anyhow!("Failed to create Imagen client: {}", e))?;

        let gen_config = build_gen_config(args.size.as_ref(), args.aspect.as_ref())?;
        let output_path = args.output.unwrap_or_else(|| {
            // Generate filename: image-hash(prompt).png
            PathBuf::from(generate_output_filename("image", &prompt, "png"))
        });
        debug!("Output path: {:?}", output_path);

        generate_single(
            &client,
            &prompt,
            output_path,
            gen_config.as_ref(),
            args.quiet,
        )
        .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_parsing() {
        let yaml = r#"
prompts:
  - name: test-image
    prompt: A beautiful sunset
    output: sunset.png
  - name: another
    prompt: A mountain landscape
  - name: with-config
    prompt: High res image
    model: 3pro
    size: 2K
    aspect: 16:9
"#;
        let parsed: PromptsFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.prompts.len(), 3);
        assert_eq!(parsed.prompts[0].name, "test-image");
        assert_eq!(parsed.prompts[0].prompt, "A beautiful sunset");
        assert_eq!(parsed.prompts[0].output, Some("sunset.png".to_string()));
        assert_eq!(parsed.prompts[1].name, "another");
        assert_eq!(parsed.prompts[1].output, None);
        assert_eq!(parsed.prompts[2].name, "with-config");
        assert_eq!(parsed.prompts[2].model, Some("3pro".to_string()));
        assert_eq!(parsed.prompts[2].size, Some("2K".to_string()));
        assert_eq!(parsed.prompts[2].aspect, Some("16:9".to_string()));
    }

    #[test]
    fn test_parse_model() {
        assert!(parse_model("2.5-flash").is_ok());
        assert!(parse_model("flash").is_ok());
        assert!(parse_model("3pro").is_ok());
        assert!(parse_model("3-pro").is_ok());
        assert!(parse_model("invalid").is_err());
    }

    #[test]
    fn test_parse_size() {
        assert!(parse_size("1K").is_ok());
        assert!(parse_size("2K").is_ok());
        assert!(parse_size("4K").is_ok());
        assert!(parse_size("8K").is_err());
    }

    #[test]
    fn test_parse_aspect() {
        assert!(parse_aspect("1:1").is_ok());
        assert!(parse_aspect("16:9").is_ok());
        assert!(parse_aspect("square").is_ok());
        assert!(parse_aspect("2:1").is_err());
    }

    #[test]
    fn test_build_gen_config() {
        // No config
        let config = build_gen_config(None, None).unwrap();
        assert!(config.is_none());

        // Size only
        let config = build_gen_config(Some(&"2K".to_string()), None).unwrap();
        assert!(config.is_some());
        assert_eq!(config.as_ref().unwrap().size, Some(ImageSize::K2));
        assert!(config.as_ref().unwrap().aspect_ratio.is_none());

        // Both
        let config = build_gen_config(Some(&"4K".to_string()), Some(&"16:9".to_string())).unwrap();
        assert!(config.is_some());
        assert_eq!(config.as_ref().unwrap().size, Some(ImageSize::K4));
        assert_eq!(
            config.as_ref().unwrap().aspect_ratio,
            Some(AspectRatio::Wide)
        );
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(
            slugify("Minimalist Futurist Poster"),
            "minimalist-futurist-poster"
        );
        assert_eq!(slugify("test--multiple---dashes"), "test-multiple-dashes");
        assert_eq!(slugify("  spaces  "), "spaces");
        assert_eq!(slugify("CamelCase123"), "camelcase123");
        // Emoji-only names should fallback to "image"
        assert_eq!(slugify("👻"), "image");
        assert_eq!(slugify("🎨🖼️"), "image");
    }

    #[test]
    fn test_generate_output_filename() {
        let filename = generate_output_filename("Test Name", "A prompt", "png");
        // Should be slug-hash.ext format
        assert!(filename.starts_with("test-name-"));
        assert!(filename.ends_with(".png"));
        // Hash should be 6 chars
        let parts: Vec<&str> = filename
            .strip_suffix(".png")
            .unwrap()
            .rsplitn(2, '-')
            .collect();
        assert_eq!(parts[0].len(), 6);

        // Same input should produce same hash
        let filename2 = generate_output_filename("Test Name", "A prompt", "png");
        assert_eq!(filename, filename2);

        // Different prompt should produce different hash
        let filename3 = generate_output_filename("Test Name", "Different prompt", "png");
        assert_ne!(filename, filename3);
    }
}
