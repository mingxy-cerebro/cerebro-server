use crate::domain::error::OmemError;

use super::service::ProcessedChunk;

pub struct PdfProcessor;

impl PdfProcessor {
    pub fn extract(data: &[u8], filename: &str) -> Result<Vec<ProcessedChunk>, OmemError> {
        match Self::extract_with_library(data, filename) {
            Ok(chunks) if !chunks.is_empty() => Ok(chunks),
            Ok(_) | Err(_) => Self::extract_with_pdftotext(data, filename),
        }
    }

    fn extract_with_library(data: &[u8], filename: &str) -> Result<Vec<ProcessedChunk>, OmemError> {
        let text = pdf_extract::extract_text_from_mem(data)
            .map_err(|e| OmemError::Internal(format!("pdf extraction failed: {e}")))?;

        if text.trim().is_empty() {
            return Ok(Vec::new());
        }

        let pages: Vec<&str> = text.split('\u{000C}').collect();
        let total_pages = pages.len();

        let chunks: Vec<ProcessedChunk> = pages
            .into_iter()
            .enumerate()
            .filter(|(_, page_text)| !page_text.trim().is_empty())
            .map(|(i, page_text)| ProcessedChunk {
                content: page_text.trim().to_string(),
                chunk_type: "pdf_page".to_string(),
                metadata: serde_json::json!({
                    "filename": filename,
                    "page": i + 1,
                    "total_pages": total_pages,
                }),
            })
            .collect();

        Ok(chunks)
    }

    fn extract_with_pdftotext(
        data: &[u8],
        filename: &str,
    ) -> Result<Vec<ProcessedChunk>, OmemError> {
        use std::io::Write;
        use std::process::Command;

        let mut tmp = tempfile::NamedTempFile::new()
            .map_err(|e| OmemError::Internal(format!("failed to create temp file: {e}")))?;
        tmp.write_all(data)
            .map_err(|e| OmemError::Internal(format!("failed to write temp file: {e}")))?;

        let output = Command::new("pdftotext").arg(tmp.path()).arg("-").output();

        match output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                if text.trim().is_empty() {
                    return Ok(vec![ProcessedChunk {
                        content: format!(
                            "[PDF file: {filename}, size: {} bytes, text extraction failed]",
                            data.len()
                        ),
                        chunk_type: "pdf_metadata".to_string(),
                        metadata: serde_json::json!({
                            "filename": filename,
                            "size_bytes": data.len(),
                            "extraction": "fallback_empty",
                        }),
                    }]);
                }
                Ok(vec![ProcessedChunk {
                    content: text.trim().to_string(),
                    chunk_type: "pdf_page".to_string(),
                    metadata: serde_json::json!({
                        "filename": filename,
                        "extraction": "pdftotext",
                    }),
                }])
            }
            _ => Ok(vec![ProcessedChunk {
                content: format!("[PDF file: {filename}, size: {} bytes]", data.len()),
                chunk_type: "pdf_metadata".to_string(),
                metadata: serde_json::json!({
                    "filename": filename,
                    "size_bytes": data.len(),
                    "extraction": "metadata_only",
                }),
            }]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_invalid_pdf_returns_metadata() {
        let data = b"not a real pdf";
        let chunks = PdfProcessor::extract(data, "fake.pdf").unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks[0].chunk_type == "pdf_metadata" || chunks[0].chunk_type == "pdf_page");
    }
}
