use crate::domain::error::OmemError;
use crate::embed::service::EmbedService;

pub struct NoopEmbedder {
    dims: usize,
}

impl NoopEmbedder {
    pub fn new(dimensions: usize) -> Self {
        Self { dims: dimensions }
    }
}

#[async_trait::async_trait]
impl EmbedService for NoopEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, OmemError> {
        Ok(texts.iter().map(|_| vec![0.0_f32; self.dims]).collect())
    }

    fn dimensions(&self) -> usize {
        self.dims
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_returns_zero_vectors() {
        let embedder = NoopEmbedder::new(1024);
        let texts = vec!["hello".to_string(), "world".to_string()];
        let result = embedder.embed(&texts).await.unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 1024);
        assert_eq!(result[1].len(), 1024);
        assert!(result[0].iter().all(|&v| v == 0.0));
        assert!(result[1].iter().all(|&v| v == 0.0));
    }

    #[tokio::test]
    async fn noop_empty_input() {
        let embedder = NoopEmbedder::new(512);
        let result = embedder.embed(&[]).await.unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn noop_dimensions() {
        let embedder = NoopEmbedder::new(768);
        assert_eq!(embedder.dimensions(), 768);
    }
}
