use std::collections::{BTreeMap, HashMap};

use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqlitePool};

use crate::domain::now_iso;
use crate::error::AppError;

pub fn default_pricing_unit() -> String {
    "per_1m_tokens".to_string()
}

pub fn default_pricing_source() -> String {
    "custom_user".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageStat {
    pub model_id: String,
    pub model_name: String,
    pub task_count: usize,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub official_cost_usd: f64,
    pub estimated_cost_usd: f64,
    pub metering_source: String, // "official" | "estimated" | "unmetered" | "mixed"
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
    pub official_cost_usd: f64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyStat {
    pub date: String,
    pub task_count: usize,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub official_cost_usd: f64,
    pub estimated_cost_usd: f64,
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
    pub official_cost_usd: f64,
    pub estimated_portion_cost_usd: f64,
    pub total_cost_usd: f64,
    pub official_task_count: usize,
    pub estimated_task_count: usize,
    pub unmetered_task_count: usize,
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
    #[serde(default)]
    pub provider_id: String,
    pub input_per_m: f64,
    pub output_per_m: f64,
    #[serde(default = "default_pricing_unit")]
    pub pricing_unit: String,
    #[serde(default)]
    pub price_per_unit: f64,
    #[serde(default = "default_pricing_source")]
    pub source: String,
    pub currency: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostEstimationResult {
    pub has_pricing: bool,
    pub currency: String,
    pub estimated_cost_min: f64,
    pub estimated_cost_max: f64,
    pub pricing_unit: String,
    pub source: String,
    pub updated_at: Option<String>,
    pub is_outdated: bool,
    pub note: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PricingRule {
    pub model_id: String,
    pub provider_id: String,
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub pricing_unit: String,
    pub price_per_unit: f64,
    pub source: String,
    pub currency: String,
    pub updated_at: String,
}

#[allow(dead_code)]
pub async fn get_summary(pool: &SqlitePool) -> Result<AnalyticsSummary, AppError> {
    get_summary_filtered(pool, None).await
}

pub async fn get_summary_filtered(
    pool: &SqlitePool,
    time_range: Option<String>,
) -> Result<AnalyticsSummary, AppError> {
    // 1. Load price overrides from database
    let pricing_rows = sqlx::query(
        "SELECT model_id, provider_id, input_per_m, output_per_m, pricing_unit, price_per_unit, source, currency, updated_at FROM model_pricing",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut custom_rules: HashMap<String, PricingRule> = HashMap::new();
    for row in pricing_rows {
        let mid: String = row.try_get("model_id").unwrap_or_default();
        let pid: String = row.try_get("provider_id").unwrap_or_default();
        let inp: f64 = row.try_get("input_per_m").unwrap_or(0.0);
        let out: f64 = row.try_get("output_per_m").unwrap_or(0.0);
        let unit: String = row
            .try_get("pricing_unit")
            .unwrap_or_else(|_| "per_1m_tokens".to_string());
        let unit_price: f64 = row.try_get("price_per_unit").unwrap_or(0.0);
        let src: String = row
            .try_get("source")
            .unwrap_or_else(|_| "custom_user".to_string());
        let curr: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "USD".to_string());
        let updated: String = row.try_get("updated_at").unwrap_or_default();

        let rule = PricingRule {
            model_id: mid.clone(),
            provider_id: pid.clone(),
            input_per_m: inp,
            output_per_m: out,
            pricing_unit: unit,
            price_per_unit: unit_price,
            source: src,
            currency: curr,
            updated_at: updated,
        };
        // Index by both model_id and provider_id+model_id
        if !pid.is_empty() {
            custom_rules.insert(format!("{pid}:{mid}"), rule.clone());
        }
        custom_rules.insert(mid, rule);
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

    // 3. Fetch tasks with selected time filter
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
    query_str.push_str(" ORDER BY created_at ASC");

    let mut rows = sqlx::query(&query_str).fetch(pool);

    let mut total_tasks = 0usize;
    let mut completed_tasks = 0;
    let mut failed_tasks = 0;
    let mut total_prompt_tokens = 0u64;
    let mut total_completion_tokens = 0u64;
    let mut total_tokens = 0u64;
    let mut official_cost_usd = 0.0f64;
    let mut estimated_cost_usd = 0.0f64;
    let mut official_task_count = 0usize;
    let mut estimated_task_count = 0usize;
    let mut unmetered_task_count = 0usize;
    let mut total_latency_sum = 0u64;
    let mut latency_count = 0usize;

    let mut modality_counts: BTreeMap<String, usize> = BTreeMap::new();
    // model_api_id -> (model_name, task_count, tokens, cost, official_cost, estimated_cost, latency_sum, latency_count, has_official, has_estimated, has_unmetered)
    #[allow(clippy::type_complexity)]
    let mut model_map: HashMap<
        String,
        (
            String,
            usize,
            u64,
            f64,
            f64,
            f64,
            u64,
            usize,
            bool,
            bool,
            bool,
        ),
    > = HashMap::new();
    // provider_id -> (provider_name, task_count, tokens, cost, official_cost, estimated_cost)
    let mut provider_map: HashMap<String, (String, usize, u64, f64, f64, f64)> = HashMap::new();
    // date -> (task_count, tokens, cost, official_cost, estimated_cost)
    let mut day_map: BTreeMap<String, (usize, u64, f64, f64, f64)> = BTreeMap::new();

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
        let mut has_official_usage = false;
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
                    let tot = usage
                        .get("total_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if p_tokens > 0 || c_tokens > 0 || tot > 0 {
                        has_official_usage = true;
                    }
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

        // Resolve pricing rule
        let pricing_rule = custom_rules
            .get(&format!("{provider_id}:{model_api_id}"))
            .or_else(|| custom_rules.get(&model_api_id))
            .or_else(|| custom_rules.get(&model_id))
            .cloned()
            .unwrap_or_else(|| default_rule_for_model(&model_api_id, &modality));

        let mut task_official_cost = 0.0f64;
        let mut task_estimated_cost = 0.0f64;
        let task_metering_type: &str;

        if has_official_usage {
            // Service provider returned exact usage
            let in_rate = pricing_rule.input_per_m;
            let out_rate = pricing_rule.output_per_m;
            task_official_cost = (p_tokens as f64 * in_rate / 1_000_000.0)
                + (c_tokens as f64 * out_rate / 1_000_000.0);
            task_metering_type = "official";
            official_task_count += 1;
        } else if status == "cancelled" || status == "failed" {
            // T14: If task was cancelled/failed without usage, do not invent cost
            task_metering_type = "unmetered";
            unmetered_task_count += 1;
        } else {
            // Task has no official token usage (e.g. image, video, audio or unmetered text)
            match modality.as_str() {
                "image" => {
                    let count = input_obj
                        .get("n")
                        .or_else(|| input_obj.get("count"))
                        .and_then(Value::as_u64)
                        .unwrap_or(1) as f64;
                    if pricing_rule.price_per_unit > 0.0 || pricing_rule.pricing_unit == "per_image"
                    {
                        let unit_price = if pricing_rule.price_per_unit > 0.0 {
                            pricing_rule.price_per_unit
                        } else {
                            0.02
                        };
                        task_estimated_cost = count * unit_price;
                        task_metering_type = "estimated";
                        estimated_task_count += 1;
                    } else {
                        task_metering_type = "unmetered";
                        unmetered_task_count += 1;
                    }
                }
                "video" => {
                    let duration_secs = input_obj
                        .get("seconds")
                        .or_else(|| input_obj.get("duration"))
                        .and_then(Value::as_f64)
                        .unwrap_or(5.0);
                    if pricing_rule.pricing_unit == "per_video_second"
                        && pricing_rule.price_per_unit > 0.0
                    {
                        task_estimated_cost = duration_secs * pricing_rule.price_per_unit;
                        task_metering_type = "estimated";
                        estimated_task_count += 1;
                    } else if pricing_rule.price_per_unit > 0.0 {
                        task_estimated_cost = pricing_rule.price_per_unit;
                        task_metering_type = "estimated";
                        estimated_task_count += 1;
                    } else {
                        task_metering_type = "unmetered";
                        unmetered_task_count += 1;
                    }
                }
                "audio" => {
                    if pricing_rule.price_per_unit > 0.0 {
                        task_estimated_cost = pricing_rule.price_per_unit;
                        task_metering_type = "estimated";
                        estimated_task_count += 1;
                    } else {
                        task_metering_type = "unmetered";
                        unmetered_task_count += 1;
                    }
                }
                _ => {
                    // Text without usage
                    let prompt_text = input_obj
                        .get("prompt")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !prompt_text.is_empty()
                        && (pricing_rule.input_per_m > 0.0 || pricing_rule.output_per_m > 0.0)
                    {
                        let est_prompt_tokens = (prompt_text.chars().count() as f64 / 3.5).max(1.0);
                        let est_out_tokens = 300.0;
                        task_estimated_cost = (est_prompt_tokens * pricing_rule.input_per_m
                            / 1_000_000.0)
                            + (est_out_tokens * pricing_rule.output_per_m / 1_000_000.0);
                        task_metering_type = "estimated";
                        estimated_task_count += 1;
                    } else {
                        task_metering_type = "unmetered";
                        unmetered_task_count += 1;
                    }
                }
            }
        }

        official_cost_usd += task_official_cost;
        estimated_cost_usd += task_estimated_cost;
        let task_total_cost = task_official_cost + task_estimated_cost;

        // Model aggregation
        let m_entry = model_map
            .entry(model_api_id.clone())
            .or_insert_with(|| (model_name, 0, 0, 0.0, 0.0, 0.0, 0, 0, false, false, false));
        m_entry.1 += 1;
        m_entry.2 += t_tokens;
        m_entry.3 += task_total_cost;
        m_entry.4 += task_official_cost;
        m_entry.5 += task_estimated_cost;
        if task_latency > 0 {
            m_entry.6 += task_latency;
            m_entry.7 += 1;
        }
        match task_metering_type {
            "official" => m_entry.8 = true,
            "estimated" => m_entry.9 = true,
            _ => m_entry.10 = true,
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
            .or_insert_with(|| (p_name, 0, 0, 0.0, 0.0, 0.0));
        p_entry.1 += 1;
        p_entry.2 += t_tokens;
        p_entry.3 += task_total_cost;
        p_entry.4 += task_official_cost;
        p_entry.5 += task_estimated_cost;

        // Date aggregation (YYYY-MM-DD)
        let date_prefix = created_at.chars().take(10).collect::<String>();
        if !date_prefix.is_empty() {
            let day_entry = day_map.entry(date_prefix).or_insert((0, 0, 0.0, 0.0, 0.0));
            day_entry.0 += 1;
            day_entry.1 += t_tokens;
            day_entry.2 += task_total_cost;
            day_entry.3 += task_official_cost;
            day_entry.4 += task_estimated_cost;
        }
    }

    let mut top_models: Vec<ModelUsageStat> = model_map
        .into_iter()
        .map(
            |(
                mid,
                (
                    name,
                    count,
                    tokens,
                    cost,
                    off_cost,
                    est_cost,
                    lat_sum,
                    lat_cnt,
                    has_off,
                    has_est,
                    has_unmet,
                ),
            )| {
                let metering_source = match (has_off, has_est, has_unmet) {
                    (true, false, false) => "official",
                    (false, true, false) => "estimated",
                    (false, false, true) => "unmetered",
                    _ => "mixed",
                };
                ModelUsageStat {
                    model_id: mid,
                    model_name: name,
                    task_count: count,
                    total_tokens: tokens,
                    cost_usd: (cost * 10000.0).round() / 10000.0,
                    official_cost_usd: (off_cost * 10000.0).round() / 10000.0,
                    estimated_cost_usd: (est_cost * 10000.0).round() / 10000.0,
                    metering_source: metering_source.to_string(),
                    avg_latency_ms: if lat_cnt > 0 {
                        lat_sum / lat_cnt as u64
                    } else {
                        0
                    },
                }
            },
        )
        .collect();
    top_models.sort_by(|a, b| b.task_count.cmp(&a.task_count));

    let mut top_providers: Vec<ProviderUsageStat> = provider_map
        .into_iter()
        .map(
            |(pid, (name, count, tokens, cost, off_cost, est_cost))| ProviderUsageStat {
                provider_id: pid,
                provider_name: name,
                task_count: count,
                total_tokens: tokens,
                cost_usd: (cost * 10000.0).round() / 10000.0,
                official_cost_usd: (off_cost * 10000.0).round() / 10000.0,
                estimated_cost_usd: (est_cost * 10000.0).round() / 10000.0,
            },
        )
        .collect();
    top_providers.sort_by(|a, b| b.task_count.cmp(&a.task_count));

    let recent_days: Vec<DailyStat> = day_map
        .into_iter()
        .map(
            |(date, (count, tokens, cost, off_cost, est_cost))| DailyStat {
                date,
                task_count: count,
                total_tokens: tokens,
                cost_usd: (cost * 10000.0).round() / 10000.0,
                official_cost_usd: (off_cost * 10000.0).round() / 10000.0,
                estimated_cost_usd: (est_cost * 10000.0).round() / 10000.0,
            },
        )
        .collect();

    let avg_latency_ms = if latency_count > 0 {
        total_latency_sum / latency_count as u64
    } else {
        0
    };

    let total_cost_usd = official_cost_usd + estimated_cost_usd;

    Ok(AnalyticsSummary {
        total_tasks,
        completed_tasks,
        failed_tasks,
        total_prompt_tokens,
        total_completion_tokens,
        total_tokens,
        official_cost_usd: (official_cost_usd * 10000.0).round() / 10000.0,
        estimated_portion_cost_usd: (estimated_cost_usd * 10000.0).round() / 10000.0,
        estimated_cost_usd: (total_cost_usd * 10000.0).round() / 10000.0,
        total_cost_usd: (total_cost_usd * 10000.0).round() / 10000.0,
        official_task_count,
        estimated_task_count,
        unmetered_task_count,
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
    set_pricing_detailed(
        pool,
        model_id,
        "",
        input_per_m,
        output_per_m,
        "per_1m_tokens",
        0.0,
        "custom_user",
        "USD",
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn set_pricing_detailed(
    pool: &SqlitePool,
    model_id: &str,
    provider_id: &str,
    input_per_m: f64,
    output_per_m: f64,
    pricing_unit: &str,
    price_per_unit: f64,
    source: &str,
    currency: &str,
) -> Result<(), AppError> {
    let now = now_iso();
    sqlx::query(
        "INSERT INTO model_pricing (model_id, provider_id, input_per_m, output_per_m, pricing_unit, price_per_unit, source, currency, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(model_id) DO UPDATE SET
            provider_id = excluded.provider_id,
            input_per_m = excluded.input_per_m,
            output_per_m = excluded.output_per_m,
            pricing_unit = excluded.pricing_unit,
            price_per_unit = excluded.price_per_unit,
            source = excluded.source,
            currency = excluded.currency,
            updated_at = excluded.updated_at",
    )
    .bind(model_id)
    .bind(provider_id)
    .bind(input_per_m)
    .bind(output_per_m)
    .bind(pricing_unit)
    .bind(price_per_unit)
    .bind(source)
    .bind(currency)
    .bind(now)
    .execute(pool)
    .await
    .map_err(AppError::db)?;

    Ok(())
}

pub async fn list_pricing(pool: &SqlitePool) -> Result<Vec<ModelPricing>, AppError> {
    let rows = sqlx::query(
        "SELECT model_id, provider_id, input_per_m, output_per_m, pricing_unit, price_per_unit, source, currency, updated_at FROM model_pricing ORDER BY model_id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::db)?;
    let mut list = Vec::new();
    for row in rows {
        list.push(ModelPricing {
            model_id: row.try_get("model_id").map_err(AppError::db)?,
            provider_id: row.try_get("provider_id").unwrap_or_default(),
            input_per_m: row.try_get("input_per_m").unwrap_or(0.0),
            output_per_m: row.try_get("output_per_m").unwrap_or(0.0),
            pricing_unit: row
                .try_get("pricing_unit")
                .unwrap_or_else(|_| "per_1m_tokens".to_string()),
            price_per_unit: row.try_get("price_per_unit").unwrap_or(0.0),
            source: row
                .try_get("source")
                .unwrap_or_else(|_| "custom_user".to_string()),
            currency: row
                .try_get("currency")
                .unwrap_or_else(|_| "USD".to_string()),
            updated_at: row.try_get("updated_at").unwrap_or_default(),
        });
    }
    Ok(list)
}

pub async fn estimate_task_cost(
    pool: &SqlitePool,
    modality: &str,
    model_id: &str,
    provider_id: Option<&str>,
    prompt_chars: Option<usize>,
    media_count: Option<usize>,
    duration_seconds: Option<f64>,
) -> Result<CostEstimationResult, AppError> {
    let pid = provider_id.unwrap_or_default();
    let row = sqlx::query(
        "SELECT model_id, provider_id, input_per_m, output_per_m, pricing_unit, price_per_unit, source, currency, updated_at
         FROM model_pricing WHERE model_id = ? OR (provider_id = ? AND model_id = ?) LIMIT 1",
    )
    .bind(model_id)
    .bind(pid)
    .bind(model_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::db)?;

    let rule = if let Some(r) = row {
        PricingRule {
            model_id: r.try_get("model_id").unwrap_or_default(),
            provider_id: r.try_get("provider_id").unwrap_or_default(),
            input_per_m: r.try_get("input_per_m").unwrap_or(0.0),
            output_per_m: r.try_get("output_per_m").unwrap_or(0.0),
            pricing_unit: r
                .try_get("pricing_unit")
                .unwrap_or_else(|_| "per_1m_tokens".to_string()),
            price_per_unit: r.try_get("price_per_unit").unwrap_or(0.0),
            source: r
                .try_get("source")
                .unwrap_or_else(|_| "custom_user".to_string()),
            currency: r.try_get("currency").unwrap_or_else(|_| "USD".to_string()),
            updated_at: r.try_get("updated_at").unwrap_or_default(),
        }
    } else {
        default_rule_for_model(model_id, modality)
    };

    let is_outdated = if !rule.updated_at.is_empty() {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&rule.updated_at) {
            let days = chrono::Utc::now().signed_duration_since(dt).num_days();
            days > 30
        } else {
            false
        }
    } else {
        false
    };

    if rule.source == "none"
        && rule.input_per_m == 0.0
        && rule.output_per_m == 0.0
        && rule.price_per_unit == 0.0
    {
        return Ok(CostEstimationResult {
            has_pricing: false,
            currency: "USD".to_string(),
            estimated_cost_min: 0.0,
            estimated_cost_max: 0.0,
            pricing_unit: "unmetered".to_string(),
            source: "none".to_string(),
            updated_at: None,
            is_outdated: false,
            note: "该模型暂未配置可靠报价，执行将标记为无计量".to_string(),
        });
    }

    match modality {
        "image" => {
            let count = media_count.unwrap_or(1) as f64;
            let unit_price = if rule.price_per_unit > 0.0 {
                rule.price_per_unit
            } else {
                0.02
            };
            let cost = count * unit_price;
            let note = if is_outdated {
                format!(
                    "预估生成 {} 张图片，单价 ${:.4}/张 (⚠️ 单价超30天未更新)",
                    count, unit_price
                )
            } else {
                format!("预估生成 {} 张图片，单价 ${:.4}/张", count, unit_price)
            };
            Ok(CostEstimationResult {
                has_pricing: true,
                currency: rule.currency,
                estimated_cost_min: (cost * 10000.0).round() / 10000.0,
                estimated_cost_max: (cost * 10000.0).round() / 10000.0,
                pricing_unit: rule.pricing_unit,
                source: rule.source,
                updated_at: Some(rule.updated_at),
                is_outdated,
                note,
            })
        }
        "video" => {
            let secs = duration_seconds.unwrap_or(5.0);
            let cost = if rule.pricing_unit == "per_video_second" && rule.price_per_unit > 0.0 {
                secs * rule.price_per_unit
            } else if rule.price_per_unit > 0.0 {
                rule.price_per_unit
            } else {
                0.15
            };
            let note = if is_outdated {
                format!(
                    "预估生成 {:.1} 秒视频，单价约 ${:.4}/秒 (⚠️ 单价超30天未更新)",
                    secs, rule.price_per_unit
                )
            } else {
                format!(
                    "预估生成 {:.1} 秒视频，单价约 ${:.4}/秒",
                    secs, rule.price_per_unit
                )
            };
            Ok(CostEstimationResult {
                has_pricing: true,
                currency: rule.currency,
                estimated_cost_min: (cost * 10000.0).round() / 10000.0,
                estimated_cost_max: (cost * 1.5 * 10000.0).round() / 10000.0,
                pricing_unit: rule.pricing_unit,
                source: rule.source,
                updated_at: Some(rule.updated_at),
                is_outdated,
                note,
            })
        }
        "audio" => {
            let unit_price = if rule.price_per_unit > 0.0 {
                rule.price_per_unit
            } else {
                0.015
            };
            Ok(CostEstimationResult {
                has_pricing: true,
                currency: rule.currency,
                estimated_cost_min: unit_price,
                estimated_cost_max: unit_price * 2.0,
                pricing_unit: rule.pricing_unit,
                source: rule.source,
                updated_at: Some(rule.updated_at),
                is_outdated,
                note: format!("音频单次调用预估单价 ${:.4}", unit_price),
            })
        }
        _ => {
            let chars = prompt_chars.unwrap_or(200) as f64;
            let est_inp_tokens = (chars / 3.5).max(10.0);
            let min_out = 200.0;
            let max_out = 1500.0;
            let min_cost = (est_inp_tokens * rule.input_per_m / 1_000_000.0)
                + (min_out * rule.output_per_m / 1_000_000.0);
            let max_cost = (est_inp_tokens * rule.input_per_m / 1_000_000.0)
                + (max_out * rule.output_per_m / 1_000_000.0);
            let note = if is_outdated {
                format!(
                    "输入: ${:.2}/1M, 输出: ${:.2}/1M (⚠️ 单价超30天未更新)",
                    rule.input_per_m, rule.output_per_m
                )
            } else {
                format!(
                    "输入: ${:.2}/1M, 输出: ${:.2}/1M",
                    rule.input_per_m, rule.output_per_m
                )
            };
            Ok(CostEstimationResult {
                has_pricing: true,
                currency: rule.currency,
                estimated_cost_min: (min_cost * 10000.0).round() / 10000.0,
                estimated_cost_max: (max_cost * 10000.0).round() / 10000.0,
                pricing_unit: "per_1m_tokens".to_string(),
                source: rule.source,
                updated_at: Some(rule.updated_at),
                is_outdated,
                note,
            })
        }
    }
}

fn default_rule_for_model(model_id: &str, modality: &str) -> PricingRule {
    let mid = model_id.to_ascii_lowercase();
    let now = now_iso();

    if modality == "image" || mid.contains("dall-e") || mid.contains("flux") || mid.contains("sdxl")
    {
        let unit_price = if mid.contains("dall-e-3") {
            0.040
        } else if mid.contains("flux") {
            0.025
        } else {
            0.020
        };
        return PricingRule {
            model_id: model_id.to_string(),
            provider_id: String::new(),
            input_per_m: 0.0,
            output_per_m: 0.0,
            pricing_unit: "per_image".to_string(),
            price_per_unit: unit_price,
            source: "official_catalog".to_string(),
            currency: "USD".to_string(),
            updated_at: now,
        };
    }

    if modality == "video"
        || mid.contains("cogvideo")
        || mid.contains("sora")
        || mid.contains("kling")
        || mid.contains("minimax")
    {
        return PricingRule {
            model_id: model_id.to_string(),
            provider_id: String::new(),
            input_per_m: 0.0,
            output_per_m: 0.0,
            pricing_unit: "per_video_second".to_string(),
            price_per_unit: 0.05,
            source: "official_catalog".to_string(),
            currency: "USD".to_string(),
            updated_at: now,
        };
    }

    if modality == "audio" || mid.contains("tts") || mid.contains("whisper") {
        return PricingRule {
            model_id: model_id.to_string(),
            provider_id: String::new(),
            input_per_m: 0.0,
            output_per_m: 0.0,
            pricing_unit: "per_call".to_string(),
            price_per_unit: 0.015,
            source: "official_catalog".to_string(),
            currency: "USD".to_string(),
            updated_at: now,
        };
    }

    // Text models
    let (inp, out) = if mid.contains("gpt-4o-mini") {
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
    };

    PricingRule {
        model_id: model_id.to_string(),
        provider_id: String::new(),
        input_per_m: inp,
        output_per_m: out,
        pricing_unit: "per_1m_tokens".to_string(),
        price_per_unit: 0.0,
        source: "estimated_default".to_string(),
        currency: "USD".to_string(),
        updated_at: now,
    }
}
