use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::domain::error::OmemError;
use crate::embed::EmbedService;
use crate::store::LanceStore;

use super::code::CodeProcessor;
use super::image::ImageProcessor;
use super::pdf::PdfProcessor;
use super::video::VideoProcessor;

pub struct MultiModalService {
    pub embed: Arc<dyn EmbedService>,
    pub store: Arc<LanceStore>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentType {
    Pdf,
    Image,
    Video,
    Code(String),
    Text,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedChunk {
    pub content: String,
    pub chunk_type: String,
    pub metadata: serde_json::Value,
}

impl MultiModalService {
    pub fn new(embed: Arc<dyn EmbedService>, store: Arc<LanceStore>) -> Self {
        Self { embed, store }
    }

    pub fn detect_content_type(filename: &str, mime: &str) -> ContentType {
        if mime.starts_with("application/pdf") || filename.ends_with(".pdf") {
            return ContentType::Pdf;
        }

        if mime.starts_with("image/")
            || filename.ends_with(".png")
            || filename.ends_with(".jpg")
            || filename.ends_with(".jpeg")
            || filename.ends_with(".gif")
            || filename.ends_with(".webp")
            || filename.ends_with(".bmp")
            || filename.ends_with(".svg")
        {
            return ContentType::Image;
        }

        if mime.starts_with("video/")
            || filename.ends_with(".mp4")
            || filename.ends_with(".avi")
            || filename.ends_with(".mov")
            || filename.ends_with(".mkv")
            || filename.ends_with(".webm")
        {
            return ContentType::Video;
        }

        if let Some(lang) = Self::detect_code_language(filename) {
            return ContentType::Code(lang);
        }

        if mime.starts_with("text/") || mime == "application/json" || mime == "application/xml" {
            return ContentType::Text;
        }

        ContentType::Unknown
    }

    fn detect_code_language(filename: &str) -> Option<String> {
        let ext = filename.rsplit('.').next()?;
        match ext.to_lowercase().as_str() {
            "rs" => Some("rust".to_string()),
            "py" | "pyw" => Some("python".to_string()),
            "js" | "mjs" | "cjs" => Some("javascript".to_string()),
            "ts" | "mts" | "cts" => Some("typescript".to_string()),
            "tsx" => Some("tsx".to_string()),
            "jsx" => Some("jsx".to_string()),
            "go" => Some("go".to_string()),
            "java" => Some("java".to_string()),
            "c" | "h" => Some("c".to_string()),
            "cpp" | "cc" | "cxx" | "hpp" => Some("cpp".to_string()),
            "rb" => Some("ruby".to_string()),
            _ => None,
        }
    }

    pub fn process_file(
        data: &[u8],
        content_type: &ContentType,
        filename: &str,
    ) -> Result<Vec<ProcessedChunk>, OmemError> {
        match content_type {
            ContentType::Pdf => PdfProcessor::extract(data, filename),
            ContentType::Image => ImageProcessor::extract(data, filename),
            ContentType::Video => VideoProcessor::extract(data, filename),
            ContentType::Code(lang) => CodeProcessor::extract(data, lang, filename),
            ContentType::Text => Self::process_text(data, filename),
            ContentType::Unknown => Self::process_text(data, filename),
        }
    }

    fn process_text(data: &[u8], filename: &str) -> Result<Vec<ProcessedChunk>, OmemError> {
        let text = String::from_utf8_lossy(data);
        if text.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![ProcessedChunk {
            content: text.to_string(),
            chunk_type: "text".to_string(),
            metadata: serde_json::json!({
                "filename": filename,
                "size_bytes": data.len(),
            }),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_pdf_by_extension() {
        assert_eq!(
            MultiModalService::detect_content_type("report.pdf", ""),
            ContentType::Pdf
        );
    }

    #[test]
    fn detect_pdf_by_mime() {
        assert_eq!(
            MultiModalService::detect_content_type("file", "application/pdf"),
            ContentType::Pdf
        );
    }

    #[test]
    fn detect_image_types() {
        assert_eq!(
            MultiModalService::detect_content_type("photo.png", ""),
            ContentType::Image
        );
        assert_eq!(
            MultiModalService::detect_content_type("photo.jpg", ""),
            ContentType::Image
        );
        assert_eq!(
            MultiModalService::detect_content_type("file", "image/jpeg"),
            ContentType::Image
        );
    }

    #[test]
    fn detect_video_types() {
        assert_eq!(
            MultiModalService::detect_content_type("clip.mp4", ""),
            ContentType::Video
        );
        assert_eq!(
            MultiModalService::detect_content_type("file", "video/mp4"),
            ContentType::Video
        );
    }

    #[test]
    fn detect_code_languages() {
        assert_eq!(
            MultiModalService::detect_content_type("main.rs", ""),
            ContentType::Code("rust".to_string())
        );
        assert_eq!(
            MultiModalService::detect_content_type("app.py", ""),
            ContentType::Code("python".to_string())
        );
        assert_eq!(
            MultiModalService::detect_content_type("index.js", ""),
            ContentType::Code("javascript".to_string())
        );
        assert_eq!(
            MultiModalService::detect_content_type("index.ts", ""),
            ContentType::Code("typescript".to_string())
        );
        assert_eq!(
            MultiModalService::detect_content_type("main.go", ""),
            ContentType::Code("go".to_string())
        );
    }

    #[test]
    fn detect_text_by_mime() {
        assert_eq!(
            MultiModalService::detect_content_type("readme", "text/plain"),
            ContentType::Text
        );
    }

    #[test]
    fn detect_unknown() {
        assert_eq!(
            MultiModalService::detect_content_type("file.xyz", "application/octet-stream"),
            ContentType::Unknown
        );
    }

    #[test]
    fn process_text_file() {
        let data = b"Hello, world!";
        let chunks =
            MultiModalService::process_file(data, &ContentType::Text, "hello.txt").unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Hello, world!");
        assert_eq!(chunks[0].chunk_type, "text");
    }

    #[test]
    fn process_empty_text() {
        let data = b"";
        let chunks =
            MultiModalService::process_file(data, &ContentType::Text, "empty.txt").unwrap();
        assert!(chunks.is_empty());
    }
}
