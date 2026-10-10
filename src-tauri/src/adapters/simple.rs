use std::time::{Duration, Instant};

use super::ProviderAdapter;
use crate::canonical::{
    AdapterContext, CanonicalRequest, CanonicalResponse, ConnectionReport, RemoteModel,
};
use crate::error::AppError;
use crate::http_util;
use async_trait::async_trait;

pub struct ProbeAdapter {
    label: &'static str,
}

pub const GENERIC_HTTP: ProbeAdapter = ProbeAdapter {
    label: "Generic HTTP",
};

#[async_trait]
impl ProviderAdapter for ProbeAdapter {
    async fn test_connection(&self, ctx: &AdapterContext) -> Result<ConnectionReport, AppError> {
        let url = ctx.provider.base_url.clone();
        let started = Instant::now();
        let response = http_util::send(
            ctx.http.get(url),
            Duration::from_millis(ctx.provider.timeout_ms),
            &ctx.provider.name,
            None,
        )
        .await?;
        let status = response.status().as_u16();
        let body = String::from_utf8_lossy(
            &http_util::read_body(response, &ctx.provider.name, None).await?,
        )
        .into_owned();
        if !(200..300).contains(&status) {
            return Err(http_util::error_from_status(
                status,
                &body,
                &ctx.provider.name,
                None,
            ));
        }
        Ok(ConnectionReport {
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            message: "已连接".to_string(),
        })
    }

    async fn list_models(&self, _ctx: &AdapterContext) -> Result<Vec<RemoteModel>, AppError> {
        Err(AppError::unsupported(format!(
            "{} 暂不支持拉取模型列表",
            self.label
        )))
    }

    async fn execute(
        &self,
        ctx: &AdapterContext,
        request: CanonicalRequest,
    ) -> Result<CanonicalResponse, AppError> {
        let modality = match request {
            CanonicalRequest::Video(payload) => {
                return super::generic_http::execute(ctx, payload).await;
            }
            CanonicalRequest::Audio(payload) => {
                return super::generic_http::execute_audio(ctx, payload).await;
            }
            CanonicalRequest::Text(_) => "文本生成",
            CanonicalRequest::Image(_) => "图像生成",
        };
        Err(AppError::unsupported(format!(
            "{modality}目前由 OpenAI、OpenAI Compatible 与 Ollama 适配器执行，{} 已可保存并测试连接",
            self.label
        )))
    }
}
