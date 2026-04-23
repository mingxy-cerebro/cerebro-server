use crate::domain::error::OmemError;

#[async_trait::async_trait]
pub trait EmbedService: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, OmemError>;
    fn dimensions(&self) -> usize;
}
