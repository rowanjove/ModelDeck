use std::path::Path;

use futures_util::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Map;
use sqlx::SqlitePool;

use crate::credentials::SecretStore;
use crate::domain::GenerateTextInput;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareInput {
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub model_ids: Vec<String>,
    pub parameters: Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareItemResult {
    pub model_id: String,
    pub model_name: String,
    pub provider_name: String,
    pub text: String,
    pub latency_ms: u64,
    pub tokens: u64,
    pub cost_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareResult {
    pub prompt: String,
    pub items: Vec<CompareItemResult>,
}

pub async fn execute(
    pool: &SqlitePool,
    http: &Client,
    secrets: &dyn SecretStore,
    data_dir: &Path,
    input: CompareInput,
) -> Result<CompareResult, AppError> {
    let prompt = input.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(AppError::validation("提示词不能为空"));
    }
    if input.model_ids.len() < 2 || input.model_ids.len() > 4 {
        return Err(AppError::validation("多模型对比必须选择 2 到 4 个模型"));
    }

    let mut futures = Vec::new();
    for mid in &input.model_ids {
        let single_input = GenerateTextInput {
            model_definition_id: mid.clone(),
            prompt: prompt.clone(),
            system_prompt: input.system_prompt.clone(),
            parameters: input.parameters.clone(),
            extra: None,
            images: vec![],
        };
        futures.push(async move {
            let res =
                crate::services::generate::execute(pool, http, secrets, data_dir, single_input)
                    .await;
            (mid.clone(), res)
        });
    }

    let results = join_all(futures).await;
    let mut items = Vec::new();
    let pricing_list = crate::services::analytics::list_pricing(pool)
        .await
        .unwrap_or_default();

    for (mid, res) in results {
        match res {
            Ok(output) => {
                let (p_tokens, c_tokens, t_tokens) = if let Some(usage) = &output.usage {
                    let p = usage.input_tokens.unwrap_or(0);
                    let c = usage.output_tokens.unwrap_or(0);
                    let t = usage.total_tokens.unwrap_or(p + c);
                    (p, c, t)
                } else {
                    let p_tokens = output
                        .raw_response
                        .get("usage")
                        .and_then(|u: &serde_json::Value| {
                            u.get("prompt_tokens")
                                .and_then(|v: &serde_json::Value| v.as_u64())
                        })
                        .unwrap_or(0);
                    let c_tokens = output
                        .raw_response
                        .get("usage")
                        .and_then(|u: &serde_json::Value| {
                            u.get("completion_tokens")
                                .and_then(|v: &serde_json::Value| v.as_u64())
                        })
                        .unwrap_or(0);
                    let t_tokens = if p_tokens + c_tokens > 0 {
                        p_tokens + c_tokens
                    } else {
                        crate::services::context::estimate_tokens(&output.text)
                    };
                    (p_tokens, c_tokens, t_tokens)
                };

                let pricing = pricing_list
                    .iter()
                    .find(|p| p.model_id == mid || p.model_id == output.model_id);
                let (input_rate, output_rate) = match pricing {
                    Some(p) => (p.input_per_m, p.output_per_m),
                    None => (2.0, 6.0),
                };

                let cost = (p_tokens as f64 * input_rate / 1_000_000.0)
                    + (c_tokens as f64 * output_rate / 1_000_000.0);

                let reasoning = if output.reasoning.trim().is_empty() {
                    None
                } else {
                    Some(output.reasoning)
                };
                let reasoning_tokens = output.usage.as_ref().and_then(|u| u.reasoning_tokens);

                items.push(CompareItemResult {
                    model_id: mid,
                    model_name: output.model_name,
                    provider_name: output.provider_name,
                    text: output.text,
                    latency_ms: output.latency_ms,
                    tokens: t_tokens,
                    cost_usd: (cost * 10000.0).round() / 10000.0,
                    reasoning,
                    reasoning_tokens,
                    error: None,
                });
            }
            Err(err) => {
                items.push(CompareItemResult {
                    model_id: mid,
                    model_name: "未知模型".to_string(),
                    provider_name: "未知".to_string(),
                    text: String::new(),
                    latency_ms: 0,
                    tokens: 0,
                    cost_usd: 0.0,
                    reasoning: None,
                    reasoning_tokens: None,
                    error: Some(err.message),
                });
            }
        }
    }

    Ok(CompareResult { prompt, items })
}
