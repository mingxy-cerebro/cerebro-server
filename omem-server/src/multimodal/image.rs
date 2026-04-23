use crate::domain::error::OmemError;

use super::service::ProcessedChunk;

pub struct ImageProcessor;

impl ImageProcessor {
    pub fn extract(data: &[u8], filename: &str) -> Result<Vec<ProcessedChunk>, OmemError> {
        match Self::extract_with_tesseract(data) {
            Ok(text) if !text.trim().is_empty() => Ok(vec![ProcessedChunk {
                content: text.trim().to_string(),
                chunk_type: "image_ocr".to_string(),
                metadata: serde_json::json!({
                    "filename": filename,
                    "size_bytes": data.len(),
                    "extraction": "tesseract",
                }),
            }]),
            _ => Ok(vec![ProcessedChunk {
                content: format!("[Image file: {filename}, size: {} bytes]", data.len()),
                chunk_type: "image_metadata".to_string(),
                metadata: serde_json::json!({
                    "filename": filename,
                    "size_bytes": data.len(),
                    "extraction": "metadata_only",
                }),
            }]),
        }
    }

    fn extract_with_tesseract(data: &[u8]) -> Result<String, OmemError> {
        use std::io::Write;
        use std::process::Command;

        let mut tmp = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .map_err(|e| OmemError::Internal(format!("failed to create temp file: {e}")))?;
        tmp.write_all(data)
            .map_err(|e| OmemError::Internal(format!("failed to write temp file: {e}")))?;

        let output = Command::new("tesseract")
            .arg(tmp.path())
            .arg("stdout")
            .output()
            .map_err(|e| OmemError::Internal(format!("tesseract not available: {e}")))?;

        if !output.status.success() {
            return Err(OmemError::Internal("tesseract failed".to_string()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_image_returns_metadata_fallback() {
        let data = b"fake image data";
        let chunks = ImageProcessor::extract(data, "photo.png").unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].chunk_type == "image_metadata" || chunks[0].chunk_type == "image_ocr");
        assert!(chunks[0].metadata["filename"] == "photo.png");
    }
}
