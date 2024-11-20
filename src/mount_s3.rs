use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::{ Client, Error as S3Error, error::SdkError, config::Region };
use derive_more::Display;
use std::{fmt, fs};
use std::path::Path;
use std::env;
use dotenv::dotenv;
use regex::Regex;
use colored::*;

use crate::{ BUCKET_NAME, REGION };
const IMAGE_DATA_DIR: &str = "./data/images";


#[derive(Debug, Display)]
#[display("ImageMetadata {{ key: {}, uid: {}, format: {} }}", key, uid, format)]
pub struct ImageMetadata {
   pub key: String,
   pub uid: String,
   pub format: String,
}

#[derive(Debug)]
pub enum MountError {
    S3Error(S3Error),
    IoError(std::io::Error),
    Other(String),
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MountError::S3Error(err) => write!(f, "S3 Error: {}", err),
            MountError::IoError(err) => write!(f, "IO Error: {}", err),
            MountError::Other(err) => write!(f, "Error: {}", err),
        }
    }
}

impl From<S3Error> for MountError {
    fn from(err: S3Error) -> Self {
        MountError::S3Error(err)
    }
}

impl From<std::io::Error> for MountError {
    fn from(err: std::io::Error) -> Self {
        MountError::IoError(err)
    }
}

impl From<String> for MountError {
    fn from(err: String) -> Self {
        MountError::Other(err)
    }
}

impl<E> From<SdkError<E>> for MountError {
    fn from(err: SdkError<E>) -> Self {
        MountError::Other(err.to_string())
    }
}

// Simple YAML parser for our specific case
fn parse_image_yaml(content: &str) -> Option<(String, String)> {
    let uid_re = Regex::new(r"uid\s*:\s*([^\n]+)").unwrap();
    let format_re = Regex::new(r"format\s*:\s*([^\n]+)").unwrap();

    let uid = uid_re
        .captures(content)
        .and_then(|cap| cap.get(1).map(|m| m.as_str().trim().to_string()));
    let format = format_re
        .captures(content)
        .and_then(|cap| cap.get(1).map(|m| m.as_str().trim().to_string()));

    match (uid, format) {
        (Some(uid), Some(format)) => Some((uid, format)),
        _ => None,
    }
}


pub struct S3Mount {
    client: Client,
}

impl S3Mount {
    pub async fn new() -> Result<Self, MountError> {
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
        // Create client and config separately

        println!("Creating S3 client...");
        let client = Client::new(&config);

        Ok(S3Mount {
            client,
        })
    }

    pub fn create_local_dir(&self, dir_path: &str) -> Result<(), MountError> {
        fs::create_dir_all(dir_path)?;
        Ok(())
    }

    pub async fn download_file(&self, key: &str, local_path: &str) -> Result<(), MountError> {

        println!("{}", format!("Downloading {} to {}", key, local_path).yellow().bold());

        // Check if file already exists
        if Path::new(local_path).exists() {
            println!("{}", "File already exists, skipping".bright_cyan().italic());
            return Ok(());
        }

        // Ensure the directory exists
        if let Some(parent) = Path::new(local_path).parent() {
            fs::create_dir_all(parent)?;
        }
        



        let get_object = self.client
            .get_object()
            .bucket(BUCKET_NAME.to_string())
            .key(key)
            .send().await?;

        let data = get_object.body.collect().await.map_err(|e| MountError::Other(e.to_string()))?;

        fs::write(local_path, data.into_bytes())?;

        Ok(())
    }

    // Function to read image metadata files
    pub fn get_image_metadata() -> Vec<ImageMetadata> {
        let mut images = Vec::new();

        match fs::read_dir(IMAGE_DATA_DIR) {
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.extension().and_then(|s| s.to_str()) == Some("yml") {
                            match fs::read_to_string(&path) {
                                Ok(content) => {
                                    if let Some((uid, format)) = parse_image_yaml(&content) {
                                        images.push(ImageMetadata {
                                            key: format!("{}.{}", uid, format),
                                            uid,
                                            format,
                                        });
                                    }
                                }
                                Err(err) => eprintln!("Error reading {}: {}", path.display(), err),
                            }
                        }
                    }
                }
            }
            Err(err) => eprintln!("Error reading metadata directory: {}", err),
        }

        images
    }
}
