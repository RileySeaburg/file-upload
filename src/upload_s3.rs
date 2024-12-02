use aws_sdk_s3::{Client, Error as S3Error, primitives::ByteStream, error::SdkError};
use std::fmt;
use std::fs;
use std::path::Path;
use mime_guess::from_path;
use image::{ImageFormat, DynamicImage};
use chrono::Local;
use serde_yaml;
use std::collections::HashMap;
use image::imageops::FilterType;

const VARIANT_SETTINGS: &[(&str, u32)] = &[
    ("mobile", 200),
    ("tablet", 400),
    ("desktop_md", 800),
    ("desktop_lg", 1200),
];

#[derive(Debug)]
pub enum UploadError {
    S3Error(S3Error),
    IoError(std::io::Error),
    ImageError(image::ImageError),
    Other(String),
}

impl fmt::Display for UploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UploadError::S3Error(err) => write!(f, "S3 Error: {}", err),
            UploadError::IoError(err) => write!(f, "IO Error: {}", err),
            UploadError::ImageError(err) => write!(f, "Image Error: {}", err),
            UploadError::Other(err) => write!(f, "Error: {}", err),
        }
    }
}

impl From<S3Error> for UploadError {
    fn from(err: S3Error) -> Self {
        UploadError::S3Error(err)
    }
}

impl From<std::io::Error> for UploadError {
    fn from(err: std::io::Error) -> Self {
        UploadError::IoError(err)
    }
}

impl From<image::ImageError> for UploadError {
    fn from(err: image::ImageError) -> Self {
        UploadError::ImageError(err)
    }
}

impl From<String> for UploadError {
    fn from(err: String) -> Self {
        UploadError::Other(err)
    }
}

impl<E> From<SdkError<E>> for UploadError {
    fn from(err: SdkError<E>) -> Self {
        UploadError::Other(err.to_string())
    }
}

#[derive(Debug, serde::Serialize)]
struct ImageMetadata {
    date: String,
    uid: String,
    width: u32,
    height: u32,
    format: String,
    alt: String,
    caption: String,
    credit: String,
}

#[derive(Debug, serde::Serialize)]
struct FileMetadata {
    date: String,
    uid: String,
    format: String,
}

pub struct S3Config {
    bucket: String,
}

impl S3Config {
    pub fn new() -> Self {
        let bucket = std::env::var("AWS_BUCKET_NAME")
            .expect("AWS_BUCKET_NAME must be set");
        
        S3Config { bucket }
    }
}

pub struct S3Upload {
    config: S3Config,
    client: Client,
}

impl S3Upload {
    pub async fn new() -> Result<Self, UploadError> {
        let config = aws_config::from_env().load().await;
        let client = Client::new(&config);
        let s3_config = S3Config::new();
        
        Ok(S3Upload {
            config: s3_config,
            client,
        })
    }

    async fn convert_jpg_to_png(&self, image_path: &Path) -> Result<String, UploadError> {
        let img = image::open(image_path)?;
        let new_path = image_path.with_extension("png");
        img.save_with_format(&new_path, ImageFormat::Png)?;
        
        if image_path.exists() {
            fs::remove_file(image_path)?;
        }
        
        Ok(new_path.to_string_lossy().into_owned())
    }

    async fn create_image_variants(&self, image_path: &Path, processed_dir: &Path) -> Result<Vec<String>, UploadError> {
        let img = image::open(image_path)?;
        let mut variant_paths = Vec::new();

        for (variant_name, width) in VARIANT_SETTINGS {
            let filename = image_path.file_stem().unwrap().to_string_lossy();
            let extension = image_path.extension().unwrap().to_string_lossy();
            let variant_filename = format!("{}_w{}.{}", filename, width, extension);
            let variant_path = processed_dir.join(&variant_filename);

            let height = (img.height() as f32 * (*width as f32 / img.width() as f32)) as u32;
            let resized = img.resize_exact(*width, height, FilterType::Lanczos3);
            resized.save(&variant_path)?;

            variant_paths.push(variant_path.to_string_lossy().into_owned());
        }

        Ok(variant_paths)
    }

    fn generate_image_metadata(&self, image: &DynamicImage, uid: &str, format: &str) -> ImageMetadata {
        ImageMetadata {
            date: Local::now().format("%Y-%m-%d %H:%M:%S -0400").to_string(),
            uid: uid.to_string(),
            width: image.width(),
            height: image.height(),
            format: format.to_string(),
            alt: String::new(),
            caption: String::new(),
            credit: String::new(),
        }
    }

    fn generate_file_metadata(&self, uid: &str, format: &str) -> FileMetadata {
        FileMetadata {
            date: Local::now().format("%Y-%m-%d %H:%M:%S -0400").to_string(),
            uid: uid.to_string(),
            format: format.to_string(),
        }
    }

    async fn write_metadata(&self, metadata: &(impl serde::Serialize + std::fmt::Debug), path: &Path) -> Result<(), UploadError> {
        let yaml = serde_yaml::to_string(metadata)
            .map_err(|e| UploadError::Other(e.to_string()))?;
        fs::write(path, yaml)?;
        Ok(())
    }

    pub async fn process_and_upload(&self, local_path: &str) -> Result<(), UploadError> {
        let path = Path::new(local_path);
        let extension = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Convert JPG to PNG if needed
        let final_path = if extension == "jpg" || extension == "jpeg" {
            self.convert_jpg_to_png(path).await?
        } else {
            local_path.to_string()
        };

        let path = Path::new(&final_path);
        let is_image = matches!(extension.as_str(), "png" | "jpg" | "jpeg");

        if is_image {
            // Process image and variants
            let processed_dir = path.parent().unwrap().join("processed");
            fs::create_dir_all(&processed_dir)?;

            let img = image::open(path)?;
            let uid = path.file_stem().unwrap().to_string_lossy();
            
            // Generate and write metadata
            let metadata = self.generate_image_metadata(&img, &uid, &extension);
            let metadata_path = Path::new("data/images").join(format!("{}.yml", uid));
            fs::create_dir_all(metadata_path.parent().unwrap())?;
            self.write_metadata(&metadata, &metadata_path).await?;

            // Create and upload variants
            let variants = self.create_image_variants(path, &processed_dir).await?;
            for variant_path in variants {
                self.upload_file(&variant_path, &Path::new(&variant_path).file_name().unwrap().to_string_lossy()).await?;
            }
        } else {
            // Handle regular files
            let uid = path.file_stem().unwrap().to_string_lossy();
            let metadata = self.generate_file_metadata(&uid, &extension);
            let metadata_path = Path::new("data/files").join(format!("{}.yml", uid));
            fs::create_dir_all(metadata_path.parent().unwrap())?;
            self.write_metadata(&metadata, &metadata_path).await?;

            self.upload_file(local_path, &format!("static/{}", path.file_name().unwrap().to_string_lossy())).await?;
        }

        Ok(())
    }

    pub async fn upload_file(&self, local_path: &str, key: &str) -> Result<(), UploadError> {
        let body = ByteStream::from_path(Path::new(local_path))
            .await
            .map_err(|e| UploadError::Other(e.to_string()))?;

        let content_type = from_path(local_path)
            .first_or_octet_stream()
            .to_string();

        self.client
            .put_object()
            .bucket(&self.config.bucket)
            .key(key)
            .body(body)
            .content_type(content_type)
            .send()
            .await?;

        Ok(())
    }

    pub async fn delete_file(&self, key: &str) -> Result<(), UploadError> {
        self.client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(key)
            .send()
            .await?;

        Ok(())
    }
}
