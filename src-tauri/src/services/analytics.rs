use std::collections::{BTreeMap, HashMap};

use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqlitePool};

use crate::domain::now_iso;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageStat {
    pub model_id: String,
    pub model_name: String,
    pub task_count: usize,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub avg_latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageStat {
    pub provider_id: String,
    pub provider_name: String,
    pub task_count: usize,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyStat {
    pub date: String,
    pub task_count: usize,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsSummary {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    pub avg_latency_ms: u64,
    pub modality_counts: BTreeMap<String, usize>,
    pub top_models: Vec<ModelUsageStat>,
    pub top_providers: Vec<ProviderUsageStat>,
    pub recent_days: Vec<DailyStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub model_id: String,
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub currency: String,
}

#[allow(dead_code)]
pub async fn get_summary(pool: &SqlitePool) -> Result<AnalyticsSummary, AppError> {
    get_summary_filtered(pool, None).await
}

pub async fn get_summary_filtered(
    pool: &SqlitePool,
    time_range: Option<String>,
) -> Result<AnalyticsSummary, AppError> {
    // 1. Load price overrides
    let pricing_rows =
        sqlx::query("SELECT model_id, input_per_m, output_per_m, currency FROM model_pricing")
            .fetch_all(pool)
            .await
            .unwrap_or_default();

    let mut custom_rates: HashMap<String, (f64, f64)> = HashMap::new();
    for row in pricing_rows {
        let mid: String = row.try_get("model_id").unwrap_or_default();
        let inp: f64 = row.try_get("input_per_m").unwrap_or(0.0);
        let out: f64 = row.try_get("output_per_m").unwrap_or(0.0);
        custom_rates.insert(mid, (inp, out));
    }

    // 2. Load provider name mapping
    let provider_rows = sqlx::query("SELECT id, name FROM providers")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let mut provider_names: HashMap<String, String> = HashMap::new();
    for prow in provider_rows {
        let pid: String = prow.try_get("id").unwrap_or_default();
        let pname: String = prow.try_get("name").unwrap_or_default();
        provider_names.insert(pid, pname);
    }

    // 3. Fetch tasks with the selected time filter and aggregate the stream
    let mut query_str = "SELECT id, modality, provider_id, model_id, status, input_json, raw_response_json, created_at, completed_at FROM generation_tasks".to_string();
    let filter = time_range.as_deref().unwrap_or("30d");
    match filter {
        "today" => {
            let today = now_iso().chars().take(10).collect::<String>();
            query_str.push_str(&format!(" WHERE created_at LIKE '{today}%'"));
        }
        "7d" => {
            let cutoff = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
            query_str.push_str(&format!(" WHERE created_at >= '{cutoff}'"));
        }
        "30d" => {
            let cutoff = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
            query_str.push_str(&format!(" WHERE created_at >= '{cutoff}'"));
        }
        "all" => {}
        _ => {
            let cutoff = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
            query_str.push_str(&format!(" WHERE created_at >= '{cutoff}'"));
        }
    }
    // The summary is a total for the selected range. A fixed row cap silently
    // under-counts older projects and makes the "all" filter incorrect.
    query_str.push_str(" ORDER BY created_at ASC");

    let mut rows = sqlx::query(&query_str).fetch(pool);

    let mut total_tasks = 0usize;
    let mut completed_tasks = 0;
    let mut failed_tasks = 0;
    let mut total_prompt_tokens = 0u64;
    let mut total_completion_tokens = 0u64;
    let mut total_tokens = 0u64;
    let mut estimated_cost_usd = 0.0f64;
    let mut total_latency_sum = 0u64;
    let mut latency_count = 0usize;

    let mut modality_counts: BTreeMap<String, usize> = BTreeMap::new();
    // model_api_id -> (model_name, count, tokens, cost, latency_sum, latency_count)
    let mut model_map: HashMap<String, (String, usize, u64, f64, u64, usize)> = HashMap::new();
    // provider_id -> (provider_name, count, tokens, cost)
    let mut provider_map: HashMap<String, (String, usize, u64, f64)> = HashMap::new();
    let mut day_map: BTreeMap<String, (usize, u64, f64)> = BTreeMap::new();

    while let Some(row) = rows.try_next().await.map_err(AppError::db)? {
        total_tasks += 1;
        let modality: String = row
            .try_get("modality")
            .unwrap_or_else(|_| "text".to_string());
        let provider_id: String = row.try_get("provider_id").unwrap_or_default();
        let model_id: String = row.try_get("model_id").unwrap_or_default();
        let status: String = row.try_get("status").unwrap_or_default();
        let input_json_str: String = row.try_get("input_json").unwrap_or_default();
        let raw_resp_str: Option<String> = row.try_get("raw_response_json").ok();
        let created_at: String = row.try_get("created_at").unwrap_or_default();
        let completed_at: Option<String> = row.try_get("completed_at").ok();

        *modality_counts.entry(modality.clone()).or_insert(0) += 1;

        if status == "completed" {
            completed_tasks += 1;
        } else if status == "failed" {
            failed_tasks += 1;
        }

        let input_obj: Value = serde_json::from_str(&input_json_str).unwrap_or(Value::Null);
        let model_api_id = input_obj
            .get("modelApiId")
            .or_else(|| input_obj.get("model_api_id"))
            .and_then(Value::as_str)
            .unwrap_or(&model_id)
            .to_string();
        let model_name = input_obj
            .get("modelName")
            .or_else(|| input_obj.get("model_name"))
            .and_then(Value::as_str)
            .unwrap_or(&model_api_id)
            .to_string();

        let mut p_tokens = 0u64;
        let mut c_tokens = 0u64;
        let mut task_latency = 0u64;

        if let Some(resp_str) = raw_resp_str {
            if let Ok(val) = serde_json::from_str::<Value>(&resp_str) {
                if let Some(usage) = val.get("usage") {
                    p_tokens = usage
                        .get("prompt_tokens")
                        .and_then(Value::as_u64)
                        .or_else(|| usage.get("input_tokens").and_then(Value::as_u64))
                        .unwrap_or(0);
                    c_tokens = usage
                        .get("completion_tokens")
                        .and_then(Value::as_u64)
                        .or_else(|| usage.get("output_tokens").and_then(Value::as_u64))
                        .unwrap_or(0);
                }
                if let Some(lat) = val
                    .get("latency_ms")
                    .or_else(|| val.get("latencyMs"))
                    .or_else(|| val.get("duration_ms"))
                    .and_then(Value::as_u64)
                {
                    task_latency = lat;
                }
            }
        }

        if task_latency == 0 {
            if let Some(comp_str) = completed_at {
                if let (Ok(c_dt), Ok(comp_dt)) = (
                    chrono::DateTime::parse_from_rfc3339(&created_at),
                    chrono::DateTime::parse_from_rfc3339(&comp_str),
                ) {
                    let diff = comp_dt.signed_duration_since(c_dt).num_milliseconds();
                    if diff > 0 {
                        task_latency = diff as u64;
                    }
                }
            }
        }

        if task_latency > 0 {
            total_latency_sum += task_latency;
            latency_count += 1;
        }

        let t_tokens = p_tokens + c_tokens;
        total_prompt_tokens += p_tokens;
        total_completion_tokens += c_tokens;
        total_tokens += t_tokens;

        let (in_rate, out_rate) = match custom_rates
            .get(&model_api_id)
            .or_else(|| custom_rates.get(&model_id))
        {
            Some(rates) => *rates,
            None => default_rates(&model_api_id),
        };

        let task_cost =
            (p_tokens as f64 * in_rate / 1_000_000.0) + (c_tokens as f64 * out_rate / 1_000_000.0);
        estimated_cost_usd += task_cost;

        // Model aggregation
        let m_entry = model_map
            .entry(model_api_id.clone())
            .or_insert_with(|| (model_name, 0, 0, 0.0, 0, 0));
        m_entry.1 += 1;
        m_entry.2 += t_tokens;
        m_entry.3 += task_cost;
        if task_latency > 0 {
            m_entry.4 += task_latency;
            m_entry.5 += 1;
        }

        // Provider aggregation
        let p_name = provider_names
            .get(&provider_id)
            .cloned()
            .unwrap_or_else(|| {
                if provider_id.is_empty() {
                    "Default Provider".to_string()
                } else {
                    provider_id.clone()
                }
            });
        let p_entry = provider_map
            .entry(provider_id.clone())
            .or_insert_with(|| (p_name, 0, 0, 0.0));
        p_entry.1 += 1;
        p_entry.2 += t_tokens;
        p_entry.3 += task_cost;

        // Date aggregation (YYYY-MM-DD)
        let date_prefix = created_at.chars().take(10).collect::<String>();
        if !date_prefix.is_empty() {
            let day_entry = day_map.entry(date_prefix).or_insert((0, 0, 0.0));
            day_entry.0 += 1;
            day_entry.1 += t_tokens;
            day_entry.2 += task_cost;
        }
    }

    let mut top_models: Vec<ModelUsageStat> = model_map
        .into_iter()
        .map(
            |(mid, (name, count, tokens, cost, lat_sum, lat_cnt))| ModelUsageStat {
                model_id: mid,
                model_name: name,
                task_count: count,
                total_tokens: tokens,
                cost_usd: (cost * 10000.0).round() / 10000.0,
                avg_latency_ms: if lat_cnt > 0 {
                    lat_sum / lat_cnt as u64
                } else {
                    0
                },
            },
        )
        .collect();
    top_models.sort_by(|a, b| b.task_count.cmp(&a.task_count));

    let mut top_providers: Vec<ProviderUsageStat> = provider_map
        .into_iter()
        .map(|(pid, (name, count, tokens, cost))| ProviderUsageStat {
            provider_id: pid,
            provider_name: name,
            task_count: count,
            total_tokens: tokens,
            cost_usd: (cost * 10000.0).round() / 10000.0,
        })
        .collect();
    top_providers.sort_by(|a, b| b.task_count.cmp(&a.task_count));

    let recent_days: Vec<DailyStat> = day_map
        .into_iter()
        .map(|(date, (count, tokens, cost))| DailyStat {
            date,
            task_count: count,
            total_tokens: tokens,
            cost_usd: (cost * 10000.0).round() / 10000.0,
        })
        .collect();

    let avg_latency_ms = if latency_count > 0 {
        total_latency_sum / latency_count as u64
    } else {
        0
    };

    Ok(AnalyticsSummary {
        total_tasks,
        completed_tasks,
        failed_tasks,
        total_prompt_tokens,
        total_completion_tokens,
        total_tokens,
        estimated_cost_usd: (estimated_cost_usd * 10000.0).round() / 10000.0,
        avg_latency_ms,
        modality_counts,
        top_models,
        top_providers,
        recent_days,
    })
}

pub async fn set_pricing(
    pool: &SqlitePool,
    model_id: &str,
    input_per_m: f64,
    output_per_m: f64,
) -> Result<(), AppError> {
    let now = now_iso();
    sqlx::query(
        "INSERT INTO model_pricing (model_id, input_per_m, output_per_m, currency, updated_at)
         VALUES (?, ?, ?, 'USD', ?)
         ON CONFLICT(model_id) DO UPDATE SET
            input_per_m = excluded.input_per_m,
            output_per_m = excluded.output_per_m,
            updated_at = excluded.updated_at",
    )
    .bind(model_id)
    .bind(input_per_m)
    .bind(output_per_m)
    .bind(now)
    .execute(pool)
    .await
    .map_err(AppError::db)?;

    Ok(())
}

pub async fn list_pricing(pool: &SqlitePool) -> Result<Vec<ModelPricing>, AppError> {
    let rows = sqlx::query("SELECT model_id, input_per_m, output_per_m, currency FROM model_pricing ORDER BY model_id ASC")
        .fetch_all(pool)
        .await
        .map_err(AppError::db)?;
    let mut list = Vec::new();
    for row in rows {
        list.push(ModelPricing {
            model_id: row.try_get("model_id").map_err(AppError::db)?,
            input_per_m: row.try_get("input_per_m").map_err(AppError::db)?,
            output_per_m: row.try_get("output_per_m").map_err(AppError::db)?,
            currency: row.try_get("currency").map_err(AppError::db)?,
        });
    }
    Ok(list)
}

fn default_rates(model_id: &str) -> (f64, f64) {
    let mid = model_id.to_ascii_lowercase();
    if mid.contains("gpt-4o-mini") {
        (0.15, 0.60)
    } else if mid.contains("gpt-4o") {
        (2.50, 10.00)
    } else if mid.contains("claude-3-5-sonnet") || mid.contains("claude-3-7-sonnet") {
        (3.00, 15.00)
    } else if mid.contains("claude-3-5-haiku") {
        (0.80, 4.00)
    } else if mid.contains("deepseek-r1") || mid.contains("reasoner") {
        (0.55, 2.19)
    } else if mid.contains("deepseek-chat") || mid.contains("deepseek-v3") {
        (0.14, 0.28)
    } else if mid.contains("gemini-2.5-pro") || mid.contains("gemini-1.5-pro") {
        (1.25, 5.00)
    } else if mid.contains("gemini-2.5-flash") || mid.contains("gemini-1.5-flash") {
        (0.075, 0.30)
    } else {
        (1.0, 2.0)
    }
}
