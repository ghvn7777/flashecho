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

use transcript_tool::imagen_api::{AspectRatio, ImageSize};
use transcript_tool::imagen_edit_api::{
    ImageEditClient, ImageEditClientConfig, ImageEditConfig, InputImage,
};

#[derive(Parser, Debug)]
#[command(name = "imagen_edit")]
#[command(version)]
#[command(about = "Edit and transform images using Gemini API")]
#[command(after_help = "EXAMPLES:
    # Single edit with CLI arguments
    imagen_edit -i photo.jpg \"Make it look like a watercolor painting\"
    imagen_edit -i face1.png -i face2.png \"An office group photo of these people\"
    imagen_edit -i img1.jpg -i img2.jpg --size 2K --aspect 16:9 \"Combine into panorama\"

    # Batch mode with YAML file
    imagen_edit --yaml edits.yaml
    imagen_edit --yaml edits.yaml --name group-photo
    imagen_edit --yaml edits.yaml -j 4")]
struct Args {
    /// Input image file(s) - can specify multiple with -i
    #[arg(short, long = "input", num_args = 1..)]
    input: Option<Vec<PathBuf>>,

    /// Text prompt describing the desired edit/transformation
    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,

    /// YAML file containing edit tasks
    #[arg(short = 'y', long)]
    yaml: Option<PathBuf>,

    /// Process only specific entry by name from YAML
    #[arg(short = 'n', long)]
    name: Option<String>,

    /// Output file path (for single edit) or directory (for YAML batch)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Image size: 1K, 2K, 4K
    #[arg(short = 's', long)]
    size: Option<String>,

    /// Aspect ratio: 1:1, 16:9, 9:16, 4:3, 3:4, 5:4
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

/// YAML file structure for batch edits
#[derive(Debug, Deserialize)]
struct EditsFile {
    edits: Vec<EditEntry>,
}

#[derive(Debug, Deserialize, Clone)]
struct EditEntry {
    name: String,
    prompt: String,
    images: Vec<String>,
    output: Option<String>,
    size: Option<String>,
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

fn parse_size(size_str: &str) -> Result<ImageSize> {
    ImageSize::from_str(size_str).map_err(|e| anyhow::anyhow!("{}", e))
}

fn parse_aspect(aspect_str: &str) -> Result<AspectRatio> {
    // Handle 5:4 aspect ratio which isn't in the standard enum
    if aspect_str == "5:4" {
        // 5:4 is close to Standard (4:3), we'll use the API value directly
        return Ok(AspectRatio::Standard);
    }
    AspectRatio::from_str(aspect_str).map_err(|e| anyhow::anyhow!("{}", e))
}

fn build_edit_config(
    size: Option<&String>,
    aspect: Option<&String>,
) -> Result<Option<ImageEditConfig>> {
    if size.is_none() && aspect.is_none() {
        return Ok(None);
    }

    let mut config = ImageEditConfig::new();

    if let Some(s) = size {
        config = config.with_size(parse_size(s)?);
    }
    if let Some(a) = aspect {
        config = config.with_aspect_ratio(parse_aspect(a)?);
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

    if slug.is_empty() {
        "edited".to_string()
    } else {
        slug
    }
}

/// Generate output filename: slug(name)-hash(name+prompt).ext
fn generate_output_filename(name: &str, prompt: &str, extension: &str) -> String {
    let slug = slugify(name);
    // Truncate slug to reasonable length
    let truncated: String = slug.chars().take(40).collect();
    let hash_input = format!("{}{}", name, prompt);
    let hash = blake3::hash(hash_input.as_bytes());
    let hash_prefix = &hash.to_hex()[..6];
    format!("{}-{}.{}", truncated, hash_prefix, extension)
}

async fn save_image(image: &transcript_tool::GeneratedImage, path: &PathBuf) -> Result<()> {
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

async fn load_images(paths: &[PathBuf]) -> Result<Vec<InputImage>> {
    let mut images = Vec::new();
    for path in paths {
        debug!("Loading image: {:?}", path);
        let image = InputImage::from_path(path)
            .await
            .with_context(|| format!("Failed to load image: {:?}", path))?;
        images.push(image);
    }
    Ok(images)
}

async fn edit_single(
    client: &ImageEditClient,
    prompt: &str,
    images: &[InputImage],
    output_path: PathBuf,
    edit_config: Option<&ImageEditConfig>,
    quiet: bool,
) -> Result<()> {
    let pb = if !quiet {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        pb.set_message("Editing image(s)...");
        pb.enable_steady_tick(Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    let result = client
        .edit_images_with_config(prompt, images, edit_config)
        .await
        .map_err(|e| anyhow::anyhow!("Image edit failed: {}", e))?;

    if let Some(pb) = pb {
        pb.finish_with_message("Image edited successfully!");
    }

    // Ensure correct extension
    let final_path = if output_path.extension().is_none() {
        output_path.with_extension(result.extension())
    } else {
        output_path
    };

    save_image(&result, &final_path).await?;

    if !quiet {
        println!("Saved: {}", final_path.display());
    }

    Ok(())
}

struct YamlEditOptions {
    api_key: String,
    yaml_path: PathBuf,
    name_filter: Option<String>,
    output_dir: PathBuf,
    default_size: Option<String>,
    default_aspect: Option<String>,
    timeout: u64,
    max_retries: u32,
    jobs: usize,
    quiet: bool,
}

/// Result of a single edit task
struct EditResult {
    name: String,
    success: bool,
    error: Option<String>,
}

async fn edit_from_yaml(opts: YamlEditOptions) -> Result<()> {
    let yaml_content = fs::read_to_string(&opts.yaml_path)
        .await
        .context("Failed to read YAML file")?;

    let edits_file: EditsFile =
        serde_yaml::from_str(&yaml_content).context("Failed to parse YAML file")?;

    let entries: Vec<EditEntry> = if let Some(ref name) = opts.name_filter {
        edits_file
            .edits
            .into_iter()
            .filter(|e| &e.name == name)
            .collect()
    } else {
        edits_file.edits
    };

    if entries.is_empty() {
        if let Some(name) = opts.name_filter {
            anyhow::bail!("No entry found with name: {}", name);
        } else {
            anyhow::bail!("No edits found in YAML file");
        }
    }

    // Ensure output directory exists
    if !opts.output_dir.exists() {
        fs::create_dir_all(&opts.output_dir)
            .await
            .context("Failed to create output directory")?;
    }

    // Get YAML file directory for resolving relative image paths
    let yaml_dir = opts
        .yaml_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let total = entries.len();
    let jobs = opts.jobs.max(1);

    if !opts.quiet {
        println!(
            "Processing {} edit(s) with {} parallel job(s)...\n",
            total, jobs
        );
    }

    // Create semaphore for concurrency control
    let semaphore = Arc::new(Semaphore::new(jobs));
    let opts = Arc::new(opts);
    let yaml_dir = Arc::new(yaml_dir);

    // Create multi-progress bar for parallel display
    let multi_progress = Arc::new(MultiProgress::new());

    // Spawn all tasks
    let mut handles = Vec::new();

    for (i, entry) in entries.into_iter().enumerate() {
        let sem = Arc::clone(&semaphore);
        let opts = Arc::clone(&opts);
        let yaml_dir = Arc::clone(&yaml_dir);
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
                pb.set_message(format!("Processing {}...", entry.name));
                pb.enable_steady_tick(Duration::from_millis(100));
                Some(pb)
            } else {
                None
            };

            // Resolve image paths relative to YAML file
            let image_paths: Vec<PathBuf> = entry
                .images
                .iter()
                .map(|img| {
                    let path = PathBuf::from(img);
                    if path.is_absolute() {
                        path
                    } else {
                        yaml_dir.join(path)
                    }
                })
                .collect();

            // Validate images exist
            for path in &image_paths {
                if !path.exists() {
                    if let Some(pb) = pb {
                        pb.finish_with_message(format!("{} failed!", entry.name));
                    }
                    return EditResult {
                        name: entry.name.clone(),
                        success: false,
                        error: Some(format!("Image not found: {:?}", path)),
                    };
                }
            }

            // Load images
            let images = match load_images(&image_paths).await {
                Ok(imgs) => imgs,
                Err(e) => {
                    if let Some(pb) = pb {
                        pb.finish_with_message(format!("{} failed!", entry.name));
                    }
                    return EditResult {
                        name: entry.name.clone(),
                        success: false,
                        error: Some(e.to_string()),
                    };
                }
            };

            // Build client
            let config = ImageEditClientConfig {
                timeout_secs: opts.timeout,
                max_retries: opts.max_retries,
            };
            let client = match ImageEditClient::with_config(opts.api_key.clone(), config) {
                Ok(c) => c,
                Err(e) => {
                    if let Some(pb) = pb {
                        pb.finish_with_message(format!("{} failed!", entry.name));
                    }
                    return EditResult {
                        name: entry.name.clone(),
                        success: false,
                        error: Some(e.to_string()),
                    };
                }
            };

            // Build edit config
            let size = entry.size.as_ref().or(opts.default_size.as_ref());
            let aspect = entry.aspect.as_ref().or(opts.default_aspect.as_ref());
            let edit_config = match build_edit_config(size, aspect) {
                Ok(c) => c,
                Err(e) => {
                    if let Some(pb) = pb {
                        pb.finish_with_message(format!("{} failed!", entry.name));
                    }
                    return EditResult {
                        name: entry.name.clone(),
                        success: false,
                        error: Some(e.to_string()),
                    };
                }
            };

            // Edit image
            match client
                .edit_images_with_config(&entry.prompt, &images, edit_config.as_ref())
                .await
            {
                Ok(result) => {
                    // Determine output filename
                    let filename = entry.output.clone().unwrap_or_else(|| {
                        generate_output_filename(&entry.name, &entry.prompt, result.extension())
                    });
                    let output_path = opts.output_dir.join(&filename);

                    match save_image(&result, &output_path).await {
                        Ok(()) => {
                            if let Some(pb) = pb {
                                pb.finish_with_message(format!("{} -> {}", entry.name, filename));
                            }
                            EditResult {
                                name: entry.name.clone(),
                                success: true,
                                error: None,
                            }
                        }
                        Err(e) => {
                            if let Some(pb) = pb {
                                pb.finish_with_message(format!("{} failed!", entry.name));
                            }
                            EditResult {
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
                    EditResult {
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
    let results: Vec<EditResult> = futures::future::join_all(handles)
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
            println!("\nFailed edits:");
            for (name, error) in &errors {
                println!("  - {}: {}", name, error);
            }
        }
    }

    if success_count == 0 && !errors.is_empty() {
        anyhow::bail!("All image edits failed");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    init_logging(args.verbose);

    // Validate arguments
    let has_cli_input = args.input.is_some() && args.prompt.is_some();
    let has_yaml = args.yaml.is_some();

    if !has_cli_input && !has_yaml {
        anyhow::bail!(
            "Either provide input images with prompt, or use --yaml file\n\nUsage:\n  imagen_edit -i image.jpg \"your prompt here\"\n  imagen_edit --yaml edits.yaml"
        );
    }

    if has_cli_input && has_yaml {
        anyhow::bail!("Cannot use both CLI input and --yaml at the same time");
    }

    if args.name.is_some() && args.yaml.is_none() {
        anyhow::bail!("--name can only be used with --yaml");
    }

    let api_key = get_api_key()?;

    if let Some(yaml_path) = args.yaml {
        // YAML batch mode
        if !yaml_path.exists() {
            anyhow::bail!("YAML file does not exist: {:?}", yaml_path);
        }

        let output_dir = args.output.unwrap_or_else(|| PathBuf::from("./output"));
        debug!("Output directory: {:?}", output_dir);

        edit_from_yaml(YamlEditOptions {
            api_key,
            yaml_path,
            name_filter: args.name,
            output_dir,
            default_size: args.size,
            default_aspect: args.aspect,
            timeout: args.timeout,
            max_retries: args.max_retries,
            jobs: args.jobs,
            quiet: args.quiet,
        })
        .await?;
    } else if let (Some(input_paths), Some(prompt)) = (args.input, args.prompt) {
        // Single edit mode via CLI
        // Validate input files exist
        for input_path in &input_paths {
            if !input_path.exists() {
                anyhow::bail!("Input file does not exist: {:?}", input_path);
            }
        }

        // Load input images
        let pb = if !args.quiet {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg}")
                    .unwrap(),
            );
            pb.set_message(format!("Loading {} input image(s)...", input_paths.len()));
            pb.enable_steady_tick(Duration::from_millis(100));
            Some(pb)
        } else {
            None
        };

        let images = load_images(&input_paths).await?;

        if let Some(ref pb) = pb {
            pb.finish_and_clear();
        }

        // Build client and config
        let config = ImageEditClientConfig {
            timeout_secs: args.timeout,
            max_retries: args.max_retries,
        };

        let client = ImageEditClient::with_config(api_key, config)
            .map_err(|e| anyhow::anyhow!("Failed to create ImageEdit client: {}", e))?;

        let edit_config = build_edit_config(args.size.as_ref(), args.aspect.as_ref())?;

        let output_path = args
            .output
            .unwrap_or_else(|| PathBuf::from(generate_output_filename("edited", &prompt, "png")));

        edit_single(
            &client,
            &prompt,
            &images,
            output_path,
            edit_config.as_ref(),
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
edits:
  - name: group-photo
    prompt: An office group photo of these people, they are making funny faces
    images:
      - face1.png
      - face2.png
      - face3.png
    output: group.png
  - name: watercolor
    prompt: Make it look like a watercolor painting
    images:
      - photo.jpg
  - name: with-config
    prompt: High quality panorama
    images:
      - img1.jpg
      - img2.jpg
    size: 2K
    aspect: 16:9
"#;
        let parsed: EditsFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.edits.len(), 3);
        assert_eq!(parsed.edits[0].name, "group-photo");
        assert_eq!(parsed.edits[0].images.len(), 3);
        assert_eq!(parsed.edits[0].output, Some("group.png".to_string()));
        assert_eq!(parsed.edits[1].name, "watercolor");
        assert_eq!(parsed.edits[1].images.len(), 1);
        assert!(parsed.edits[1].output.is_none());
        assert_eq!(parsed.edits[2].size, Some("2K".to_string()));
        assert_eq!(parsed.edits[2].aspect, Some("16:9".to_string()));
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Make it watercolor"), "make-it-watercolor");
        assert_eq!(slugify("test--multiple---dashes"), "test-multiple-dashes");
        assert_eq!(slugify(""), "edited");
    }

    #[test]
    fn test_generate_output_filename() {
        let filename = generate_output_filename("group-photo", "A sunset photo", "png");
        assert!(filename.starts_with("group-photo-"));
        assert!(filename.ends_with(".png"));
        // Hash should be 6 chars before extension
        let parts: Vec<&str> = filename
            .strip_suffix(".png")
            .unwrap()
            .rsplitn(2, '-')
            .collect();
        assert_eq!(parts[0].len(), 6);

        // Same input should produce same hash
        let filename2 = generate_output_filename("group-photo", "A sunset photo", "png");
        assert_eq!(filename, filename2);

        // Different prompt should produce different hash
        let filename3 = generate_output_filename("group-photo", "Different prompt", "png");
        assert_ne!(filename, filename3);
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
        assert!(parse_aspect("5:4").is_ok());
        assert!(parse_aspect("2:1").is_err());
    }

    #[test]
    fn test_build_edit_config() {
        // No config
        let config = build_edit_config(None, None).unwrap();
        assert!(config.is_none());

        // With size
        let config = build_edit_config(Some(&"2K".to_string()), None).unwrap();
        assert!(config.is_some());
        assert_eq!(config.as_ref().unwrap().size, Some(ImageSize::K2));

        // With both
        let config = build_edit_config(Some(&"4K".to_string()), Some(&"16:9".to_string())).unwrap();
        assert!(config.is_some());
        assert_eq!(config.as_ref().unwrap().size, Some(ImageSize::K4));
        assert_eq!(
            config.as_ref().unwrap().aspect_ratio,
            Some(AspectRatio::Wide)
        );
    }
}
