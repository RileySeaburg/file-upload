use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::ObjectCannedAcl;
use aws_sdk_s3::Client;
use dotenv::dotenv;
use image::imageops::FilterType;
use image::ImageFormat;
use lazy_static::lazy_static;
use mime_guess::from_path as mime_from_path;
use neon::prelude::*;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::runtime::Runtime;
use colored::*;
use chrono::Local;
use urlencoding::encode;
mod mount_s3;
use mount_s3::S3Mount;

pub const REGION: &str = "us-east-1";
pub const BUCKET_NAME: &str = "digitalgov";
const INBOX_DIR: &str = "content/uploads/_inbox";
const WORKING_IMAGES_DIR: &str = "content/uploads/_working-images/to-process";
const WORKING_FILES_DIR: &str = "content/uploads/_working-files/to-process";
const IMAGE_S3_PREFIX: &str = "";
const STATIC_S3_PREFIX: &str = "static/";
const LOCAL_IMAGE_DIR: &str = "./assets/s3-images";

// Image variant settings
lazy_static! {
    static ref VARIANT_SETTINGS: HashMap<&'static str, VariantSetting> = {
        let mut m = HashMap::new();
        m.insert(
            "mobile",
            VariantSetting {
                width: 200,
            },
        );
        m.insert(
            "tablet",
            VariantSetting {
                width: 400,
            },
        );
        m.insert(
            "desktop_md",
            VariantSetting {
                width: 800,
            },
        );
        m.insert(
            "desktop_lg",
            VariantSetting {
                width: 1200
            },
        );
        m
    };
}

/// Settings for an image variant.
struct VariantSetting {
    width: u32,
}

// Return a global tokio runtime or create one if it doesn't exist.
fn runtime() -> &'static Runtime {
    static RUNTIME: OnceCell<Runtime> = OnceCell::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("Failed to create Tokio runtime"))
}

fn is_image(file_path: &Path) -> bool {
    file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map_or(false, |ext| {
            matches!(
                ext.to_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tiff" | "webp"
            )
        })
}

fn is_valid_file_type(path: &Path) -> bool {
    let extension = path.extension().and_then(OsStr::to_str).unwrap_or("").to_lowercase();
    matches!(
        extension.as_str(),
        // Images
        "jpg" |
            "jpeg" |
            "png" |
            "gif" |
            "bmp" |
            "tiff" |
            "webp" |
            // Documents
            "doc" |
            "docx" |
            "pdf" |
            "txt" |
            "rtf" |
            "xls" |
            "xlsx" |
            "csv" |
            "ppt" |
            "pptx" |
            // Other common formats
            "zip" |
            "rar" |
            "7z"
    )
}

fn sanitize_filename(filename: &str) -> String {
    filename
        .to_lowercase()
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

/// Moves files from inbox to appropriate working directories
fn prepare_working_directories() -> Result<(), Box<dyn Error + Send + Sync>> {
    let inbox = Path::new(INBOX_DIR);
    if !inbox.exists() {
        println!("Inbox directory not found at {:?}", inbox);
        return Ok(());
    }

    // Create working directories if they don't exist
    fs::create_dir_all(WORKING_IMAGES_DIR)?;
    fs::create_dir_all(WORKING_FILES_DIR)?;

    let files: Vec<_> = fs::read_dir(inbox)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let path = entry.path();
            path.is_file() && is_valid_file_type(&path) && 
            path.file_name().map_or(false, |name| name != "__add image or static files to this folder__")
        })
        .collect();

    for entry in files {
        let path = entry.path();
        let target_dir = if is_image(&path) {
            WORKING_IMAGES_DIR
        } else {
            WORKING_FILES_DIR
        };

        let file_name = path.file_name().unwrap();
        let sanitized_name = sanitize_filename(file_name.to_str().unwrap());
        let target_path = Path::new(target_dir).join(&sanitized_name);
        
        // Move file to appropriate working directory
        fs::rename(&path, &target_path)?;
        println!("Moved {:?} to {:?}", path, target_path);
    }

    Ok(())
}

/// Converts a JPG image to PNG format
async fn convert_jpg_to_png(image_path: &Path) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    println!("Converting image {:?} to PNG", image_path);
    let img = image::open(image_path)?;
    let output_path = image_path.with_extension("png");
    img.save_with_format(&output_path, ImageFormat::Png)?;
    
    if image_path.exists() {
        fs::remove_file(image_path)?;
    }
    
    Ok(output_path)
}

/// Resizes an image while maintaining its aspect ratio.
pub fn resize_image(
    image_path: &Path,
    output_path: &Path,
    width: u32
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let img = image::open(image_path)?;
    let aspect_ratio = (img.height() as f32) / (img.width() as f32);
    let height = ((width as f32) * aspect_ratio).round() as u32;
    let resized_img = img.resize_exact(width, height, FilterType::CatmullRom);
    resized_img.save(output_path)?;
    Ok(())
}

/// Generates and writes YML metadata for an image
fn write_image_metadata(uid: &str, width: u32, height: u32, format: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("Generating metadata for image - dimensions: {}x{}", width, height);
    let metadata = format!(
        r#"
# https://s3.amazonaws.com/digitalgov/{uid}.{format}
# Image shortcode: {{{{ img src="{uid}" }}}}
date     :  {}
uid      :  {}
width    :  {}
height   :  {}
format   :  {}

# REQUIRED alternative text for accessibility.
# Keep within 150 characters. https://capitalizemytitle.com/character-counter/ will count characters.
alt      :  ""

# Caption text appears below the image; usually the attribution for stock images.
# Must be different from the alt text.
caption  :  ""

# Credit text appears after the caption text, separated by an m-dash.
# Example https://digital.gov/2023/12/08/making-gsa-public-art-collection-more-accessible/ 
credit   :  ""
"#,
        Local::now().format("%Y-%m-%d %H:%M:%S -0400"),
        uid,
        width,
        height,
        format
    );

    fs::create_dir_all("data/images")?;
    fs::write(format!("data/images/{}.yml", uid), metadata)?;
    Ok(())
}

/// Generates and writes YML metadata for a file
fn write_file_metadata(uid: &str, format: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let metadata = format!(
        r#"
# https://s3.amazonaws.com/digitalgov/static/{uid}.{format}
# File shortcode: {{{{ asset-static file="{uid}.{format}" label="{uid} ({format})" }}}}
date     :  {}
uid      :  {}
format   :  {}
"#,
        Local::now().format("%Y-%m-%d %H:%M:%S -0400"),
        uid,
        format
    );

    fs::create_dir_all("data/files")?;
    fs::write(format!("data/files/{}.yml", uid), metadata)?;
    Ok(())
}

/// Uploads a file to an Amazon S3 bucket.
pub async fn upload_to_s3(
    file_path: &Path,
    key: &str,
    content_type: Option<&str>
) -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("Attempting to upload file: {:?}", file_path);

    // Load .env file
    dotenv().ok();

    let region_provider = RegionProviderChain::first_try(Region::new(REGION.to_string()))
        .or_default_provider()
        .or_else(Region::new("us-east-1"));

    println!("Loading AWS config...");
    let mut config_loader = aws_config::from_env().region(region_provider);

    // Check for credentials in .env
    if let (Ok(access_key), Ok(secret_key)) = (
        env::var("AWS_ACCESS_KEY_ID"),
        env::var("AWS_SECRET_ACCESS_KEY"),
    ) {
        println!("Using credentials from .env file");
        let creds = Credentials::new(access_key, secret_key, None, None, "dotenv");
        config_loader = config_loader.credentials_provider(creds);
    } else {
        println!("No credentials in .env, falling back to default credential provider chain");
    }

    let config = config_loader.load().await;

    println!("Creating S3 client...");
    let client = Client::new(&config);

    let file = fs::read(file_path)?;
    let stream = ByteStream::from(file);

    let request = client
        .put_object()
        .bucket(BUCKET_NAME)
        .key(key)
        .body(stream)
        .content_type(content_type.unwrap_or("application/octet-stream"))
        .acl(ObjectCannedAcl::PublicRead);

    println!("Uploading file: {:?} to S3 key: {}", file_path, key);
    request.send().await?;
    println!(
        "Upload completed. File should be accessible at: https://s3.amazonaws.com/{}/{}",
        BUCKET_NAME,
        key
    );

    Ok(())
}

pub async fn process_and_upload_file(file_path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    let file_name = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid file name")?;
    
    // Sanitize the filename
    let sanitized_name = sanitize_filename(file_name);
    let content_type = mime_from_path(file_path).first_raw();

    if is_image(file_path) {
        // Debug: Print file size
        let metadata = fs::metadata(file_path)?;
        println!("Original file size: {} bytes", metadata.len());

        // Convert JPG to PNG if needed
        let file_path = if file_path.extension().and_then(|e| e.to_str()) == Some("jpg") 
            || file_path.extension().and_then(|e| e.to_str()) == Some("jpeg") {
            convert_jpg_to_png(file_path).await?
        } else {
            file_path.to_path_buf()
        };

        let file_stem = Path::new(&sanitized_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Invalid file name")?;
        let extension = file_path
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("Invalid file extension")?;

        // Read and validate image dimensions
        let img = image::open(&file_path)?;
        let (width, height) = (img.width(), img.height());
        println!("Original image dimensions: {}x{}", width, height);
        
        if width < 100 || height < 100 {
            println!("Warning: Image dimensions seem unusually small. This might indicate an issue with the image file.");
            // Optional: Return an error if dimensions are too small
            // return Err("Image dimensions are too small".into());
        }

        // Upload the original file first
        let original_s3_key = format!("{}{}.{}", IMAGE_S3_PREFIX, file_stem, extension);
        upload_to_s3(&file_path, &original_s3_key, content_type).await?;
        println!("Uploaded original file to S3: {}", original_s3_key);

        // Generate metadata
        println!("Generating metadata for image - dimensions: {}x{}", width, height);
        write_image_metadata(file_stem, width, height, extension)?;

        // Then process and upload resized versions
        for (variant_name, variant) in VARIANT_SETTINGS.iter() {
            let output_filename = format!("{}_w{}.{}", file_stem, variant.width, extension);
            let output_path = Path::new(WORKING_IMAGES_DIR).join(&output_filename);

            resize_image(&file_path, &output_path, variant.width)?;

            // Verify resized dimensions
            if let Ok(resized_img) = image::open(&output_path) {
                println!(
                    "Resized image dimensions for {} variant: {}x{}", 
                    variant_name,
                    resized_img.width(),
                    resized_img.height()
                );
            }

            let s3_key = format!("{}{}", IMAGE_S3_PREFIX, output_filename);
            upload_to_s3(&output_path, &s3_key, content_type).await?;
            println!("Uploaded resized file to S3: {}", s3_key);

            fs::remove_file(output_path)?;
        }
    } else {
        // For non-image files, upload directly to the STATIC_S3_PREFIX
        let s3_key = format!("{}{}", STATIC_S3_PREFIX, sanitized_name);
        println!("Uploading non-image file to S3: {}", s3_key);
        upload_to_s3(file_path, &s3_key, content_type).await?;

        // Generate metadata for the file
        let file_stem = Path::new(&sanitized_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Invalid file name")?;
        let extension = file_path
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("Invalid file extension")?;
        write_file_metadata(file_stem, extension)?;
    }

    Ok(())
}

async fn process_and_upload_all() -> Result<String, Box<dyn Error + Send + Sync>> {
    println!("Starting file upload process...");

    // First, move files from inbox to working directories
    prepare_working_directories()?;

    let image_dir = Path::new(WORKING_IMAGES_DIR);
    let file_dir = Path::new(WORKING_FILES_DIR);

    let mut total_count = 0;
    let mut processed_count = 0;

    for dir in &[image_dir, file_dir] {
        if !dir.exists() {
            println!("Working directory not found at {:?}", dir);
            continue;
        }

        let files: Vec<_> = fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let path = entry.path();
                path.is_file() && is_valid_file_type(&path)
            })
            .collect();

        total_count += files.len();

        println!("Found {} valid files in {:?}.", files.len(), dir);

        for entry in files {
            let path = entry.path();
            match process_and_upload_file(&path).await {
                Ok(_) => {
                    processed_count += 1;
                    println!("Successfully processed and uploaded: {:?}", path);
                    // Remove the original file after successful upload
                    if let Err(e) = fs::remove_file(&path) {
                        println!("Error removing file {:?}: {}", path, e);
                    }
                }
                Err(e) => {
                    println!("Error processing file {:?}: {}", path, e);
                    // Continue with the next file
                }
            }
        }
    }

    // Cleanup: remove working directories
    let directories_to_remove = [
        Path::new("content/uploads/_working-images"),
        Path::new("content/uploads/_working-files"),
    ];

    for dir in &directories_to_remove {
        if dir.exists() {
            if let Err(e) = fs::remove_dir_all(dir) {
                println!("Error removing directory {:?}: {}", dir, e);
            }
        }
    }

    println!("Upload process completed successfully.");
    if total_count == 0 {
        Ok("No valid files to process.".into())
    } else {
        Ok(format!(
            "Successfully processed and uploaded {} out of {} files.",
            processed_count,
            total_count
        ))
    }
}

fn process_and_upload_js(mut cx: FunctionContext) -> JsResult<JsString> {
    let result = runtime().block_on(async {
        match process_and_upload_all().await {
            Ok(message) => message,
            Err(e) => format!("Error: {}", e),
        }
    });

    Ok(cx.string(result))
}

fn mkdir_and_download_all_images_from_s3(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let result = runtime().block_on(async {
        println!("{}", "Creating S3 mount...".yellow().bold());
        match S3Mount::new().await {
            Ok(mount) => {
                if let Err(e) = mount.create_local_dir(LOCAL_IMAGE_DIR) {
                    println!("Error creating local directory: {}", e);
                    return Err(());
                }
                
                let keys = S3Mount::get_image_metadata();
            
                for key in keys {
                    let local_path = format!("{}/{}", LOCAL_IMAGE_DIR, key.key);
                    if let Err(e) = mount.download_file(&key.key, &local_path).await {
                        println!("Error downloading file: {}", e);
                    }
                }

                println!("{}", "All images downloaded successfully".green());

                Ok(())
            }
            Err(e) => {
                println!("Error creating S3 mount: {}", e);
                Err(())
            }
        }
    });

    Ok(JsBoolean::new(&mut cx, result.is_ok()))
}

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("mkdir_and_download_files", mkdir_and_download_all_images_from_s3)?;
    cx.export_function("upload", process_and_upload_js)?;
    Ok(())
}
