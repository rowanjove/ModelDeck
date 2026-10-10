use async_trait::async_trait;

use crate::canonical::{
    AdapterContext, CanonicalRequest, CanonicalResponse, ConnectionReport, LiveDelta, RemoteModel,
    TextResult,
};
use crate::domain::ProviderType;
use crate::error::AppError;

mod anthropic;
mod comfyui;
mod exchange;
mod fal;
mod gemini;
mod generic_http;
mod media_job;
mod ollama;
mod openai;
mod openai_audio;
mod openai_image;
mod replicate;
mod simple;

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    async fn test_connection(&self, ctx: &AdapterContext) -> Result<ConnectionReport, AppError>;
    async fn list_models(&self, ctx: &AdapterContext) -> Result<Vec<RemoteModel>, AppError>;
    async fn execute(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
    ) -> Result<CanonicalResponse, AppError>;

    async fn stream(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
        deltas: &(dyn Fn(LiveDelta) + Send + Sync),
    ) -> Result<TextResult, AppError> {
        match self.execute(ctx, request).await? {
            CanonicalResponse::Text(result) => {
                if !result.reasoning.is_empty() {
                    deltas(LiveDelta::Reasoning(result.reasoning.clone()));
                }
                if !result.text.is_empty() {
                    deltas(LiveDelta::Text(result.text.clone()));
                }
                Ok(result)
            }
            CanonicalResponse::Image(_) => Err(AppError::unsupported("图像生成不支持流式输出")),
            CanonicalResponse::Video(_) => Err(AppError::unsupported("视频生成不支持流式输出")),
            CanonicalResponse::Audio(_) => Err(AppError::unsupported("音频生成不支持流式输出")),
        }
    }
}

pub(crate) use openai::data_url_parts;
pub(crate) use openai_audio::{generate_srt, generate_vtt};

pub fn adapter_for(kind: ProviderType) -> &'static dyn ProviderAdapter {
    match kind {
        ProviderType::Openai | ProviderType::OpenaiCompatible => &openai::OPENAI,
        ProviderType::Ollama => &ollama::OLLAMA,
        ProviderType::Comfyui => &comfyui::COMFYUI,
        ProviderType::GenericHttp => &simple::GENERIC_HTTP,
        ProviderType::Anthropic => &anthropic::ANTHROPIC,
        ProviderType::Gemini => &gemini::GEMINI,
        ProviderType::Replicate => &replicate::REPLICATE,
        ProviderType::Fal => &fal::FAL,
    }
}
