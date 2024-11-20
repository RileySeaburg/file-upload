use aws_sdk_s3::{Client, Error as S3Error, primitives::ByteStream, error::SdkError};
use std::fmt;
use std::fs;
use std::path::Path;
use mime_guess::from_path;

#[derive(Debug)]
pub enum UploadError {
    S3Error(S3Error),
    IoError(std::io::Error),
    Other(String),
}

impl fmt::Display for UploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UploadError::S3Error(err) => write!(f, "S3 Error: {}", err),
            UploadError::IoError(err) => write!(f, "IO Error: {}", err),
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

pub struct S3Config {
    bucket: String,
}

impl S3Config {
    pub fn new() -> Self {
        // Load bucket name from environment variable
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

    pub async fn upload_file(&self, local_path: &str, key: &str) -> Result<(), UploadError> {
        // Check if file exists
        if !Path::new(local_path).exists() {
            return Err(UploadError::Other(format!("File not found: {}", local_path)));
        }

        // Read file content
        let body = ByteStream::from_path(Path::new(local_path))
            .await
            .map_err(|e| UploadError::Other(e.to_string()))?;

        // Guess MIME type
        let content_type = from_path(local_path)
            .first_or_octet_stream()
            .to_string();

        // Upload to S3
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

    pub async fn upload_data(&self, data: Vec<u8>, key: &str, content_type: &str) -> Result<(), UploadError> {
        let body = ByteStream::from(data);

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
