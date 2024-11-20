use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::ObjectCannedAcl;
use aws_sdk_s3::Client;
use dotenv::dotenv;
use image::imageops::FilterType;
use lazy_static::lazy_static;
use mime_guess::from_path as mime_from_path;
use neon::prelude::*;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use tokio::runtime::Runtime;
use colored::*;
mod mount_s3;
use mount_s3::S3Mount;

const REGION: &str = "us-east-1";
const BUCKET_NAME: &str = "digitalgov";
const INBOX_DIR: &str = "content/uploads/_working-images/to-process";
const FILE_INBOX_DIR: &str = "content/uploads/_working-files/to-process/";
const IMAGE_S3_PREFIX: &str = "";
const STATIC_S3_PREFIX: &str = "static/";
const LOCAL_IMAGE_DIR: &str = "./assets/s3-images";

// Image variant settings
lazy_static! {
    static ref VARIANT_SETTINGS: HashMap<&'static str, VariantSetting> = {
        let mut m = HashMap::new();
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
    if
        let (Ok(access_key), Ok(secret_key)) = (
            env::var("AWS_ACCESS_KEY_ID"),
            env::var("AWS_SECRET_ACCESS_KEY"),
        )
    {
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
    let content_type = mime_from_path(file_path).first_raw();

    if is_image(file_path) {
        let file_stem = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Invalid file name")?;
        let extension = file_path
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("Invalid file extension")?;

        // Upload the original file first
        let original_s3_key = format!("{}{}.{}", IMAGE_S3_PREFIX, file_stem, extension);
        upload_to_s3(file_path, &original_s3_key, content_type).await?;
        println!("Uploaded original file to S3: {}", original_s3_key);

        // Then process and upload resized versions
        for (_, variant) in VARIANT_SETTINGS.iter() {
            let output_filename = format!("{}_w{}.{}", file_stem, variant.width, extension);
            let output_path = Path::new(INBOX_DIR).join(&output_filename);

            resize_image(file_path, &output_path, variant.width)?;

            let s3_key = format!("{}{}", IMAGE_S3_PREFIX, output_filename);
            upload_to_s3(&output_path, &s3_key, content_type).await?;
            println!("Uploaded resized file to S3: {}", s3_key);

            fs::remove_file(output_path)?;
        }
    } else {
        // For non-image files, upload directly to the STATIC_S3_PREFIX
        let s3_key = format!("{}{}", STATIC_S3_PREFIX, file_name);
        println!("Uploading non-image file to S3: {}", s3_key);
        upload_to_s3(file_path, &s3_key, content_type).await?;
    }

    Ok(())
}

async fn process_and_upload_all() -> Result<String, Box<dyn Error + Send + Sync>> {
    let image_inbox = Path::new(INBOX_DIR);
    let file_inbox = Path::new(FILE_INBOX_DIR);

    let mut total_count = 0;
    let mut processed_count = 0;

    for inbox in &[image_inbox, file_inbox] {
        if !inbox.exists() {
            println!("Inbox directory not found at {:?}", inbox);
            continue;
        }

        let files: Vec<_> = fs
            ::read_dir(inbox)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let path = entry.path();
                path.is_file() && is_valid_file_type(&path)
            })
            .collect();

        total_count += files.len();

        println!("Found {} valid files in inbox {:?}.", files.len(), inbox);

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

    if total_count == 0 {
        return Ok("No valid files to process.".into());
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

    Ok(
        format!(
            "Successfully processed and uploaded {} out of {} valid files.",
            processed_count,
            total_count
        )
    )
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
