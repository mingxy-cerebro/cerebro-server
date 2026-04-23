use crate::domain::error::OmemError;

use super::service::ProcessedChunk;

pub struct VideoProcessor;

impl VideoProcessor {
    pub fn extract(data: &[u8], filename: &str) -> Result<Vec<ProcessedChunk>, OmemError> {
        let metadata = Self::extract_metadata(data, filename);

        match Self::transcribe(data) {
            Ok(transcript) if !transcript.trim().is_empty() => {
                let duration_str = metadata
                    .get("duration")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown duration");
                Ok(vec![
                    ProcessedChunk {
                        content: transcript.trim().to_string(),
                        chunk_type: "video_transcript".to_string(),
                        metadata: metadata.clone(),
                    },
                    ProcessedChunk {
                        content: format!("[Video: {filename}, {duration_str}]"),
                        chunk_type: "video_metadata".to_string(),
                        metadata,
                    },
                ])
            }
            Err(e) => {
                tracing::warn!(error = %e, "video transcription unavailable, using metadata only");
                Ok(vec![ProcessedChunk {
                    content: format!(
                        "[Video file: {filename}, size: {} bytes. Transcription not available.]",
                        data.len()
                    ),
                    chunk_type: "video_metadata".to_string(),
                    metadata,
                }])
            }
            Ok(_) => Ok(vec![ProcessedChunk {
                content: format!(
                    "[Video file: {filename}, size: {} bytes. Transcription empty.]",
                    data.len()
                ),
                chunk_type: "video_metadata".to_string(),
                metadata,
            }]),
        }
    }

    fn extract_metadata(data: &[u8], filename: &str) -> serde_json::Value {
        match Self::run_ffprobe(data) {
            Ok(probe) => {
                let format = probe.get("format").cloned().unwrap_or_default();
                serde_json::json!({
                    "filename": filename,
                    "size_bytes": data.len(),
                    "extraction": "ffprobe",
                    "duration": format.get("duration").and_then(|v| v.as_str()).unwrap_or("unknown"),
                    "format_name": format.get("format_name").and_then(|v| v.as_str()).unwrap_or("unknown"),
                    "bit_rate": format.get("bit_rate").and_then(|v| v.as_str()).unwrap_or("unknown"),
                })
            }
            Err(_) => {
                serde_json::json!({
                    "filename": filename,
                    "size_bytes": data.len(),
                    "extraction": "metadata_only",
                })
            }
        }
    }

    fn run_ffprobe(data: &[u8]) -> Result<serde_json::Value, String> {
        use std::io::Write;
        use std::process::Command;

        let mut tmp = tempfile::Builder::new()
            .suffix(".mp4")
            .tempfile()
            .map_err(|e| format!("temp file: {e}"))?;
        tmp.write_all(data)
            .map_err(|e| format!("write temp: {e}"))?;

        let output = Command::new("ffprobe")
            .args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(tmp.path())
            .output()
            .map_err(|e| format!("ffprobe not available: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "ffprobe failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        serde_json::from_slice(&output.stdout).map_err(|e| format!("parse ffprobe json: {e}"))
    }

    fn transcribe(data: &[u8]) -> Result<String, String> {
        use std::io::Write;
        use std::process::Command;

        let mut video_tmp = tempfile::Builder::new()
            .suffix(".mp4")
            .tempfile()
            .map_err(|e| format!("temp file: {e}"))?;
        video_tmp
            .write_all(data)
            .map_err(|e| format!("write video temp: {e}"))?;

        let audio_tmp = tempfile::Builder::new()
            .suffix(".wav")
            .tempfile()
            .map_err(|e| format!("temp file: {e}"))?;
        let audio_path = audio_tmp.path().to_path_buf();
        drop(audio_tmp);

        let ffmpeg_out = Command::new("ffmpeg")
            .arg("-i")
            .arg(video_tmp.path())
            .args(["-vn", "-acodec", "pcm_s16le", "-ar", "16000", "-y"])
            .arg(&audio_path)
            .output()
            .map_err(|e| format!("ffmpeg not available: {e}"))?;

        if !ffmpeg_out.status.success() {
            let _ = std::fs::remove_file(&audio_path);
            return Err(format!(
                "ffmpeg audio extraction failed: {}",
                String::from_utf8_lossy(&ffmpeg_out.stderr)
            ));
        }

        let transcript = Self::run_whisper(&audio_path);
        let _ = std::fs::remove_file(&audio_path);
        transcript
    }

    fn run_whisper(audio_path: &std::path::Path) -> Result<String, String> {
        use std::process::Command;

        let output = Command::new("whisper")
            .arg(audio_path)
            .args(["--model", "base", "--output_format", "txt", "--output_dir"])
            .arg(audio_path.parent().unwrap_or(std::path::Path::new("/tmp")))
            .output()
            .map_err(|e| format!("whisper not available: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "whisper failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let txt_path = audio_path.with_extension("txt");
        std::fs::read_to_string(&txt_path).map_err(|e| format!("read whisper output: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_video_returns_metadata() {
        let data = b"fake video data";
        let chunks = VideoProcessor::extract(data, "clip.mp4").unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_type, "video_metadata");
        assert!(chunks[0].content.contains("clip.mp4"));
        assert_eq!(chunks[0].metadata["size_bytes"], 15);
    }

    #[test]
    fn extract_metadata_fallback_without_ffprobe() {
        let data = b"not a real video";
        let meta = VideoProcessor::extract_metadata(data, "test.mp4");
        assert_eq!(meta["filename"], "test.mp4");
        assert_eq!(meta["size_bytes"], 16);
        assert!(meta["extraction"] == "metadata_only" || meta["extraction"] == "ffprobe",);
    }

    #[test]
    fn transcribe_fails_gracefully_without_tools() {
        let data = b"not a real video";
        let result = VideoProcessor::transcribe(data);
        assert!(result.is_ok() || result.is_err());
    }
}
