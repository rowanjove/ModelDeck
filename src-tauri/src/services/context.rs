use crate::canonical::TextTurn;
use crate::domain::MediaRef;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextStrategy {
    All,
    Recent,
    Tokens,
    Summary,
}

impl ContextStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Recent => "recent",
            Self::Tokens => "tokens",
            Self::Summary => "summary",
        }
    }
}

pub fn parse_strategy(value: &str) -> Result<ContextStrategy, AppError> {
    match value {
        "all" => Ok(ContextStrategy::All),
        "recent" => Ok(ContextStrategy::Recent),
        "tokens" => Ok(ContextStrategy::Tokens),
        "summary" => Ok(ContextStrategy::Summary),
        _ => Err(AppError::validation("未知的上下文策略")),
    }
}

pub fn auto_title(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = collapsed.chars().take(24).collect();
    if title.is_empty() {
        "新对话".to_string()
    } else {
        title
    }
}

pub fn normalize_limit(strategy: ContextStrategy, limit: u32) -> u32 {
    match strategy {
        ContextStrategy::All => 0,
        ContextStrategy::Recent | ContextStrategy::Summary => limit.clamp(1, 200),
        ContextStrategy::Tokens => limit.clamp(128, 200_000),
    }
}

pub fn estimate_tokens(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    let mut ascii = 0u64;
    let mut other = 0u64;
    for character in text.chars() {
        if character.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    (ascii.div_ceil(4) + other).max(1)
}

#[derive(Debug, Clone)]
pub struct ContextItem {
    pub id: String,
    pub role: String,
    pub text: String,
    pub images: Vec<String>,
    pub is_pinned: bool,
}

pub struct ContextPlan {
    pub older: Vec<ContextItem>,
    pub turns: Vec<TextTurn>,
    pub summary: Option<String>,
    pub tokens: u64,
}

pub fn plan_context(
    messages: &[ContextItem],
    strategy: ContextStrategy,
    limit: u32,
    system_prompt: &str,
    cached_summary: Option<&str>,
    cached_until: Option<&str>,
) -> ContextPlan {
    let limit = normalize_limit(strategy, limit);
    let (older, selected, summary) = match strategy {
        ContextStrategy::All => (Vec::new(), messages.to_vec(), None),
        ContextStrategy::Recent => (Vec::new(), tail_with_pins(messages, limit as usize), None),
        ContextStrategy::Tokens => (
            Vec::new(),
            fit_tokens_with_pins(messages, system_prompt, u64::from(limit)),
            None,
        ),
        ContextStrategy::Summary => {
            split_summary_with_pins(messages, limit as usize, cached_summary, cached_until)
        }
    };
    let summary_text = summary.as_deref().unwrap_or("");
    let mut tokens = estimate_tokens(system_prompt) + estimate_tokens(summary_text);
    for item in &selected {
        tokens += estimate_tokens(&item.text) + item.images.len() as u64 * 256 + 4;
    }
    ContextPlan {
        older,
        turns: selected.iter().map(to_turn).collect(),
        summary,
        tokens,
    }
}

pub fn compose_system(system: &str, summary: Option<&str>) -> Option<String> {
    let mut parts = Vec::new();
    if !system.trim().is_empty() {
        parts.push(system.trim().to_string());
    }
    if let Some(summary) = summary.map(str::trim).filter(|text| !text.is_empty()) {
        parts.push(format!("此前对话摘要：\n{summary}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

pub fn transcript(items: &[ContextItem]) -> String {
    items
        .iter()
        .map(|item| {
            let role = if item.role == "assistant" {
                "助手"
            } else {
                "用户"
            };
            let images = if item.images.is_empty() {
                String::new()
            } else {
                format!("\n[图像 {} 张]", item.images.len())
            };
            format!("{role}：{}{images}", item.text.trim())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn tail_with_pins(messages: &[ContextItem], count: usize) -> Vec<ContextItem> {
    if messages.is_empty() {
        return Vec::new();
    }
    let count = count.max(1).min(messages.len());
    let mut selected_indices = std::collections::BTreeSet::new();

    // 1. 优先保留所有 pinned 消息
    for (i, msg) in messages.iter().enumerate() {
        if msg.is_pinned {
            selected_indices.insert(i);
        }
    }

    // 2. 从后往前选取未 pin 消息填满保留窗口
    let mut added = 0;
    for i in (0..messages.len()).rev() {
        if selected_indices.contains(&i) {
            continue;
        }
        if selected_indices.len() >= count && added > 0 {
            break;
        }
        selected_indices.insert(i);
        added += 1;
        if selected_indices.len() >= count {
            break;
        }
    }

    // 3. 始终确保最后一条（最新）消息被选中
    selected_indices.insert(messages.len() - 1);

    selected_indices
        .into_iter()
        .map(|i| messages[i].clone())
        .collect()
}

fn fit_tokens_with_pins(messages: &[ContextItem], system: &str, budget: u64) -> Vec<ContextItem> {
    if messages.is_empty() {
        return Vec::new();
    }
    let mut selected_indices = std::collections::BTreeSet::new();
    let mut used = estimate_tokens(system);

    // 1. 优先将所有 pinned 消息计入预算并选中
    for (i, msg) in messages.iter().enumerate() {
        if msg.is_pinned {
            let cost = estimate_tokens(&msg.text) + msg.images.len() as u64 * 256 + 4;
            used = used.saturating_add(cost);
            selected_indices.insert(i);
        }
    }

    // 2. 从后往前容纳未 pin 消息
    for i in (0..messages.len()).rev() {
        if selected_indices.contains(&i) {
            continue;
        }
        let msg = &messages[i];
        let cost = estimate_tokens(&msg.text) + msg.images.len() as u64 * 256 + 4;
        if !selected_indices.is_empty() && used.saturating_add(cost) > budget {
            break;
        }
        used = used.saturating_add(cost);
        selected_indices.insert(i);
    }

    // 3. 始终确保至少保留最后一条最新消息
    if selected_indices.is_empty() {
        selected_indices.insert(messages.len() - 1);
    }

    selected_indices
        .into_iter()
        .map(|i| messages[i].clone())
        .collect()
}

fn split_summary_with_pins(
    messages: &[ContextItem],
    keep: usize,
    cached_summary: Option<&str>,
    cached_until: Option<&str>,
) -> (Vec<ContextItem>, Vec<ContextItem>, Option<String>) {
    if messages.is_empty() {
        return (Vec::new(), Vec::new(), None);
    }
    let keep = keep.max(1).min(messages.len().max(1));
    let split = messages.len().saturating_sub(keep);

    let mut older = Vec::new();
    let mut recent_indices = std::collections::BTreeSet::new();

    // 最近 keep 条消息全部入选 recent
    for i in split..messages.len() {
        recent_indices.insert(i);
    }

    // 之前被 pin 的消息也直接入选 recent，未 pin 的历史消息进入 older 生成摘要
    for (i, msg) in messages[..split].iter().enumerate() {
        if msg.is_pinned {
            recent_indices.insert(i);
        } else {
            older.push(msg.clone());
        }
    }

    let recent = recent_indices
        .into_iter()
        .map(|i| messages[i].clone())
        .collect();

    if older.is_empty() {
        return (Vec::new(), recent, None);
    }

    let covered = cached_until == older.last().map(|item| item.id.as_str());
    if covered {
        let summary = cached_summary
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned);
        if summary.is_some() {
            return (Vec::new(), recent, summary);
        }
    }
    (older, recent, None)
}

fn to_turn(item: &ContextItem) -> TextTurn {
    TextTurn::plain(
        item.role.clone(),
        item.text.clone(),
        item.images.iter().map(MediaRef::from_path).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, role: &str, text: &str) -> ContextItem {
        ContextItem {
            id: id.to_string(),
            role: role.to_string(),
            text: text.to_string(),
            images: Vec::new(),
            is_pinned: false,
        }
    }

    fn pinned_item(id: &str, role: &str, text: &str) -> ContextItem {
        ContextItem {
            id: id.to_string(),
            role: role.to_string(),
            text: text.to_string(),
            images: Vec::new(),
            is_pinned: true,
        }
    }

    #[test]
    fn recent_keeps_only_the_tail() {
        let messages = vec![
            item("1", "user", "obsolete-topic"),
            item("2", "assistant", "old"),
            item("3", "user", "current-topic"),
        ];
        let plan = plan_context(&messages, ContextStrategy::Recent, 1, "", None, None);
        assert_eq!(plan.turns.len(), 1);
        assert_eq!(plan.turns[0].text, "current-topic");
    }

    #[test]
    fn recent_preserves_pinned_messages() {
        let messages = vec![
            pinned_item("1", "user", "important-context"),
            item("2", "assistant", "old"),
            item("3", "user", "current-topic"),
        ];
        let plan = plan_context(&messages, ContextStrategy::Recent, 1, "", None, None);
        assert_eq!(plan.turns.len(), 2);
        assert_eq!(plan.turns[0].text, "important-context");
        assert_eq!(plan.turns[1].text, "current-topic");
    }

    #[test]
    fn token_budget_drops_the_oldest_turn() {
        let messages = vec![
            item("1", "user", &"旧".repeat(200)),
            item("2", "user", "新"),
        ];
        let plan = plan_context(&messages, ContextStrategy::Tokens, 128, "", None, None);
        assert_eq!(plan.turns.len(), 1);
        assert_eq!(plan.turns[0].text, "新");
    }

    #[test]
    fn summary_reuses_cache_until_the_covered_message() {
        let messages = vec![
            item("1", "user", "旧"),
            item("2", "assistant", "答"),
            item("3", "user", "新"),
        ];
        let cached = plan_context(
            &messages,
            ContextStrategy::Summary,
            1,
            "",
            Some("已记住旧目标"),
            Some("2"),
        );
        assert!(cached.older.is_empty());
        assert_eq!(cached.summary.as_deref(), Some("已记住旧目标"));
        assert_eq!(cached.turns.len(), 1);

        let missed = plan_context(
            &messages,
            ContextStrategy::Summary,
            1,
            "",
            Some("过期"),
            Some("1"),
        );
        assert_eq!(missed.older.len(), 2);
        assert!(missed.summary.is_none());
    }

    #[test]
    fn counts_cjk_more_heavily_than_ascii() {
        assert!(estimate_tokens("你好") >= 2);
        assert_eq!(estimate_tokens("abcd"), 1);
    }

    #[test]
    fn titles_use_the_first_user_text() {
        assert_eq!(auto_title("  hello   world "), "hello world");
        assert_eq!(auto_title(&"题".repeat(30)).chars().count(), 24);
        assert_eq!(auto_title("   "), "新对话");
    }
}
