use serde_json::{Map, Value};

use crate::domain::{
    ConstraintDefinition, ParameterDefinition, ParameterType, TransportDefinition,
};
use crate::error::AppError;

const INPUT_KEYS: [&str; 7] = [
    "firstFrame",
    "lastFrame",
    "referenceImages",
    "referenceVideos",
    "sourceImage",
    "mask",
    "referenceAudio",
];

pub fn validate_definition(
    parameters: &mut [ParameterDefinition],
    constraints: &mut [ConstraintDefinition],
    transport: &mut Option<TransportDefinition>,
    capabilities: &Value,
) -> Result<(), AppError> {
    if !capabilities.is_object() {
        return Err(AppError::validation("能力声明必须是对象"));
    }
    let mut seen = Vec::new();
    for parameter in parameters.iter_mut() {
        parameter.key = parameter.key.trim().to_string();
        parameter.label = parameter.label.trim().to_string();
        if parameter.label.is_empty() {
            return Err(AppError::validation("参数名称不能为空"));
        }
        if !is_key(&parameter.key) {
            return Err(AppError::validation(format!(
                "参数键「{}」只能使用字母、数字和下划线，并且以字母或下划线开头",
                parameter.key
            )));
        }
        if seen.contains(&parameter.key) {
            return Err(AppError::validation(format!(
                "参数键重复: {}",
                parameter.key
            )));
        }
        seen.push(parameter.key.clone());
        validate_parameter_shape(parameter)?;
    }
    for constraint in constraints.iter_mut() {
        match constraint {
            ConstraintDefinition::MutuallyExclusive { fields } => {
                fields
                    .iter_mut()
                    .for_each(|field| *field = field.trim().to_string());
                if fields.len() < 2 {
                    return Err(AppError::validation("互斥约束至少要包含两个参数"));
                }
                let mut unique = fields.clone();
                unique.sort();
                unique.dedup();
                if unique.len() != fields.len() {
                    return Err(AppError::validation("互斥约束里有重复参数"));
                }
                for field in fields {
                    if !seen.iter().any(|key| key == field) && !INPUT_KEYS.contains(&field.as_str())
                    {
                        return Err(AppError::validation(format!(
                            "互斥约束引用了不存在的参数: {field}"
                        )));
                    }
                }
            }
            ConstraintDefinition::Requires {
                field,
                required_fields,
            } => {
                *field = field.trim().to_string();
                required_fields
                    .iter_mut()
                    .for_each(|item| *item = item.trim().to_string());
                if !seen.iter().any(|key| key == field) && !INPUT_KEYS.contains(&field.as_str()) {
                    return Err(AppError::validation(format!(
                        "依赖约束引用了不存在的参数: {field}"
                    )));
                }
                if required_fields.is_empty() {
                    return Err(AppError::validation("依赖约束至少需要指定一个依赖字段"));
                }
                for rf in required_fields {
                    if rf == field {
                        return Err(AppError::validation("参数不能依赖自身"));
                    }
                    if !seen.iter().any(|key| key == rf) && !INPUT_KEYS.contains(&rf.as_str()) {
                        return Err(AppError::validation(format!(
                            "依赖约束引用了不存在的依赖参数: {rf}"
                        )));
                    }
                }
            }
            ConstraintDefinition::ConditionalVisibility {
                field,
                depends_on,
                equals: _,
                not_equals: _,
            } => {
                *field = field.trim().to_string();
                *depends_on = depends_on.trim().to_string();
                if !seen.iter().any(|key| key == field) {
                    return Err(AppError::validation(format!(
                        "条件可见性约束引用了不存在的参数: {field}"
                    )));
                }
                if !seen.iter().any(|key| key == depends_on) {
                    return Err(AppError::validation(format!(
                        "条件可见性约束依赖了不存在的参数: {depends_on}"
                    )));
                }
                if field == depends_on {
                    return Err(AppError::validation("参数不能以自身为可见性条件"));
                }
            }
        }
    }
    if let Some(value) = transport {
        value.chat_path = clean_path(value.chat_path.clone(), "chatPath")?;
        value.models_path = clean_path(value.models_path.clone(), "modelsPath")?;
        value.image_path = clean_path(value.image_path.clone(), "imagePath")?;
        value.image_edit_path = clean_path(value.image_edit_path.clone(), "imageEditPath")?;
        validate_workflow(value)?;
        validate_http(value)?;
        if value.chat_path.is_none()
            && value.models_path.is_none()
            && value.image_path.is_none()
            && value.image_edit_path.is_none()
            && value.workflow.is_none()
            && value.bindings.is_none()
            && value.http.is_none()
        {
            *transport = None;
        }
    }
    defaults_respect_constraints(parameters, constraints)?;
    Ok(())
}

fn defaults_respect_constraints(
    definitions: &[ParameterDefinition],
    constraints: &[ConstraintDefinition],
) -> Result<(), AppError> {
    let mut values = Map::new();
    for definition in definitions {
        if let Some(default) = &definition.default {
            if !is_empty_value(Some(default)) {
                values.insert(definition.key.clone(), default.clone());
            }
        }
    }
    for constraint in constraints {
        match constraint {
            ConstraintDefinition::MutuallyExclusive { fields } => {
                let present: Vec<&str> = fields
                    .iter()
                    .filter(|field| values.contains_key(field.as_str()))
                    .map(String::as_str)
                    .collect();
                if present.len() > 1 {
                    return Err(AppError::validation(format!(
                        "默认值使 {} 同时存在，违反互斥约束",
                        present.join("、")
                    )));
                }
            }
            ConstraintDefinition::Requires { .. }
            | ConstraintDefinition::ConditionalVisibility { .. } => {}
        }
    }
    Ok(())
}

fn validate_parameter_shape(parameter: &mut ParameterDefinition) -> Result<(), AppError> {
    if let Some(step) = parameter.step {
        if step <= 0.0 {
            return Err(AppError::validation(format!(
                "参数「{}」的步长必须大于 0",
                parameter.label
            )));
        }
    }
    if let (Some(min), Some(max)) = (parameter.min, parameter.max) {
        if min > max {
            return Err(AppError::validation(format!(
                "参数「{}」的最小值不能大于最大值",
                parameter.label
            )));
        }
    }
    if parameter.kind == ParameterType::Select {
        let options = parameter.options.get_or_insert_with(Vec::new);
        if options.is_empty() {
            return Err(AppError::validation(format!(
                "参数「{}」至少需要一个选项",
                parameter.label
            )));
        }
        for option in options.iter_mut() {
            option.label = option.label.trim().to_string();
            if option.label.is_empty() {
                return Err(AppError::validation(format!(
                    "参数「{}」的选项名称不能为空",
                    parameter.label
                )));
            }
            if !matches!(option.value, Value::String(_) | Value::Number(_)) {
                return Err(AppError::validation(format!(
                    "参数「{}」的选项值只能是字符串或数字",
                    parameter.label
                )));
            }
        }
    } else {
        parameter.options = None;
    }
    if let Some(default) = &parameter.default {
        if is_empty_value(Some(default)) {
            parameter.default = None;
        } else {
            validate_value(parameter, default)?;
        }
    }
    Ok(())
}

fn is_parameter_visible(
    key: &str,
    values: &Map<String, Value>,
    constraints: &[ConstraintDefinition],
) -> bool {
    for constraint in constraints {
        if let ConstraintDefinition::ConditionalVisibility {
            field,
            depends_on,
            equals,
            not_equals,
        } = constraint
        {
            if field == key {
                let dep_val = values.get(depends_on);
                if let Some(eq) = equals {
                    if dep_val != Some(eq) {
                        return false;
                    }
                }
                if let Some(neq) = not_equals {
                    if dep_val == Some(neq) {
                        return false;
                    }
                }
                if equals.is_none() && not_equals.is_none() && is_empty_value(dep_val) {
                    return false;
                }
            }
        }
    }
    true
}

pub fn normalize_parameters(
    definitions: &[ParameterDefinition],
    input: &Map<String, Value>,
    constraints: &[ConstraintDefinition],
) -> Result<Map<String, Value>, AppError> {
    let known: Vec<&str> = definitions.iter().map(|item| item.key.as_str()).collect();
    for key in input.keys() {
        if !known.contains(&key.as_str()) {
            return Err(AppError::validation(format!("未知参数: {key}")));
        }
    }

    let mut preliminary = Map::new();
    for definition in definitions {
        if let Some(val) = input
            .get(&definition.key)
            .filter(|v| !is_empty_value(Some(v)))
        {
            preliminary.insert(definition.key.clone(), val.clone());
        } else if let Some(val) = &definition.default {
            if !is_empty_value(Some(val)) {
                preliminary.insert(definition.key.clone(), val.clone());
            }
        }
    }

    let mut values = Map::new();
    for definition in definitions {
        if !is_parameter_visible(&definition.key, &preliminary, constraints) {
            continue;
        }

        let provided = input
            .get(&definition.key)
            .filter(|value| !is_empty_value(Some(value)));
        let chosen = if let Some(value) = provided {
            value
        } else if definition
            .default
            .as_ref()
            .is_some_and(|value| !is_empty_value(Some(value)))
        {
            definition.default.as_ref().expect("default checked")
        } else if definition.required == Some(true) {
            return Err(AppError::validation(format!(
                "参数「{}」是必填项",
                definition.label
            )));
        } else {
            continue;
        };
        validate_value(definition, chosen)?;
        values.insert(definition.key.clone(), chosen.clone());
    }

    for constraint in constraints {
        match constraint {
            ConstraintDefinition::MutuallyExclusive { fields } => {
                let present: Vec<&str> = fields
                    .iter()
                    .filter(|field| {
                        values
                            .get(*field)
                            .is_some_and(|value| !is_empty_value(Some(value)))
                    })
                    .map(String::as_str)
                    .collect();
                if present.len() > 1 {
                    return Err(AppError::validation(format!(
                        "{} 不能同时设置",
                        present.join("、")
                    )));
                }
            }
            ConstraintDefinition::Requires {
                field,
                required_fields,
            } => {
                let is_active = match values.get(field) {
                    Some(Value::Bool(false)) => false,
                    Some(val) => !is_empty_value(Some(val)),
                    None => false,
                };
                if is_active {
                    for rf in required_fields {
                        let rf_active = match values.get(rf) {
                            Some(Value::Bool(false)) => false,
                            Some(val) => !is_empty_value(Some(val)),
                            None => false,
                        };
                        if !rf_active {
                            let field_label = definitions
                                .iter()
                                .find(|d| d.key == *field)
                                .map(|d| d.label.as_str())
                                .unwrap_or(field.as_str());
                            let rf_label = definitions
                                .iter()
                                .find(|d| d.key == *rf)
                                .map(|d| d.label.as_str())
                                .unwrap_or(rf.as_str());
                            return Err(AppError::validation(format!(
                                "参数「{}」需要同时提供「{}」",
                                field_label, rf_label
                            )));
                        }
                    }
                }
            }
            ConstraintDefinition::ConditionalVisibility { .. } => {}
        }
    }
    Ok(values)
}

pub fn reject_exclusive(
    constraints: &[ConstraintDefinition],
    present: &[&str],
) -> Result<(), AppError> {
    for constraint in constraints {
        if let ConstraintDefinition::MutuallyExclusive { fields } = constraint {
            let hit: Vec<&str> = fields
                .iter()
                .map(String::as_str)
                .filter(|field| present.contains(field))
                .collect();
            if hit.len() > 1 {
                return Err(AppError::validation(format!(
                    "{} 不能同时设置",
                    hit.join("、")
                )));
            }
        }
    }
    Ok(())
}

pub fn validate_rel_path(path: &str, label: &str) -> Result<(), AppError> {
    if path.is_empty() {
        return Err(AppError::validation(format!("{label} 不能为空")));
    }
    if path.contains("://")
        || path.starts_with("//")
        || path.contains("..")
        || path.contains('\\')
        || path.contains(' ')
        || path.contains('?')
        || path.len() > 200
    {
        return Err(AppError::validation(format!(
            "{label} 必须是不含查询参数的相对路径"
        )));
    }
    Ok(())
}

fn validate_workflow(transport: &mut TransportDefinition) -> Result<(), AppError> {
    if transport.workflow.as_ref().is_some_and(Value::is_null) {
        transport.workflow = None;
    }
    if let Some(workflow) = &transport.workflow {
        if !workflow.is_object() {
            return Err(AppError::validation("ComfyUI Workflow 必须是对象"));
        }
        let size = serde_json::to_string(workflow)
            .map(|text| text.len())
            .unwrap_or(usize::MAX);
        if size > 1_000_000 {
            return Err(AppError::validation("ComfyUI Workflow 过大"));
        }
    }
    if transport
        .bindings
        .as_ref()
        .is_some_and(|items| items.is_empty())
    {
        transport.bindings = None;
    }
    if transport
        .bindings
        .as_ref()
        .is_some_and(|items| !items.is_empty())
        && transport.workflow.is_none()
    {
        return Err(AppError::validation("工作流绑定需要 Workflow"));
    }
    let Some(bindings) = transport.bindings.as_mut() else {
        return Ok(());
    };
    if bindings.len() > 64 {
        return Err(AppError::validation("工作流绑定过多"));
    }
    for binding in bindings.iter_mut() {
        binding.key = binding.key.trim().to_string();
        binding.node = binding.node.trim().to_string();
        binding.input = binding.input.trim().to_string();
        if binding.key.is_empty() || binding.node.is_empty() || binding.input.is_empty() {
            return Err(AppError::validation("工作流绑定的键、节点和输入不能为空"));
        }
        if binding.key.chars().count() > 80
            || binding.node.chars().count() > 80
            || binding.input.chars().count() > 80
        {
            return Err(AppError::validation("工作流绑定过长"));
        }
    }
    Ok(())
}

fn validate_http(transport: &mut TransportDefinition) -> Result<(), AppError> {
    let Some(recipe) = transport.http.as_mut() else {
        return Ok(());
    };
    recipe.method = recipe.method.trim().to_ascii_uppercase();
    if recipe.method != "GET" && recipe.method != "POST" {
        return Err(AppError::validation("HTTP 方法只能是 GET 或 POST"));
    }
    recipe.path = recipe.path.trim().trim_start_matches('/').to_string();
    validate_rel_path(&recipe.path, "请求路径")?;
    if recipe.headers.len() > 20 {
        return Err(AppError::validation("HTTP 头过多"));
    }
    for header in &mut recipe.headers {
        header.name = header.name.trim().to_string();
        header.value = header.value.trim().to_string();
        if header.name.is_empty() || header.value.is_empty() {
            return Err(AppError::validation("HTTP 头的名称和值不能为空"));
        }
        if header.name.len() > 40
            || header.name.contains(' ')
            || header.name.contains(':')
            || !header.name.is_ascii()
        {
            return Err(AppError::validation("HTTP 头名称无效"));
        }
        if header.value.chars().count() > 300 {
            return Err(AppError::validation("HTTP 头的值过长"));
        }
        let mut header_parts = header.value.split_whitespace();
        let bearer_api_key = matches!(
            (header_parts.next(), header_parts.next(), header_parts.next()),
            (Some(scheme), Some(token), None)
                if scheme.eq_ignore_ascii_case("Bearer") && token == "{{apiKey}}"
        );
        let uses_api_key_placeholder = header.value == "{{apiKey}}" || bearer_api_key;
        if crate::redact::is_sensitive_key(&header.name) && !uses_api_key_placeholder {
            return Err(AppError::validation(
                "敏感 HTTP 头必须使用 {{apiKey}}，不要把密钥写进模型模板",
            ));
        }
        if header.value.contains("sk-") || header.value.contains("sk_") {
            return Err(AppError::validation("请用 {{apiKey}}，不要把密钥写进模板"));
        }
        if header.value.to_ascii_lowercase().starts_with("bearer ") {
            let token = header.value.split_whitespace().nth(1).unwrap_or("");
            if token != "{{apiKey}}" {
                return Err(AppError::validation("请用 {{apiKey}}，不要把密钥写进模板"));
            }
        }
    }
    if recipe.body.as_ref().is_some_and(Value::is_null) {
        recipe.body = None;
    }
    if let Some(body) = &recipe.body {
        if !body.is_object() {
            return Err(AppError::validation("HTTP 正文必须是对象"));
        }
        validate_template_secrets(body)?;
        let size = serde_json::to_string(body)
            .map(|text| text.len())
            .unwrap_or(usize::MAX);
        if size > 100_000 {
            return Err(AppError::validation("HTTP 正文过大"));
        }
    }
    recipe.result_path = recipe.result_path.trim().to_string();
    require_json_path(&recipe.result_path, "结果路径")?;
    if recipe
        .task_id_path
        .as_ref()
        .is_some_and(|text| text.trim().is_empty())
    {
        recipe.task_id_path = None;
    }
    if let Some(path) = recipe.task_id_path.as_mut() {
        *path = path.trim().to_string();
        require_json_path(path, "任务编号路径")?;
    }
    if recipe
        .result_kind
        .as_ref()
        .is_some_and(|text| text.trim().is_empty())
    {
        recipe.result_kind = None;
    }
    if let Some(kind) = recipe.result_kind.as_mut() {
        *kind = kind.trim().to_ascii_lowercase();
        if kind != "url" && kind != "base64" && kind != "binary" && kind != "text" {
            return Err(AppError::validation(
                "结果类型只能是 url、base64、binary 或 text",
            ));
        }
    }
    if let Some(poll) = recipe.poll.as_mut() {
        poll.method = poll.method.trim().to_ascii_uppercase();
        if poll.method != "GET" && poll.method != "POST" {
            return Err(AppError::validation("轮询方法只能是 GET 或 POST"));
        }
        poll.path = poll.path.trim().trim_start_matches('/').to_string();
        let check = poll.path.replace("{{taskId}}", "task");
        validate_rel_path(&check, "轮询路径")?;
        poll.status_path = poll.status_path.trim().to_string();
        require_json_path(&poll.status_path, "状态路径")?;
        poll.success = poll.success.trim().to_string();
        poll.failure = poll.failure.trim().to_string();
        if poll.success.is_empty() || poll.failure.is_empty() {
            return Err(AppError::validation("轮询需要成功和失败状态"));
        }
        if poll.success == poll.failure {
            return Err(AppError::validation("成功和失败状态不能相同"));
        }
        if let Some(interval) = poll.interval_ms {
            if !(200..=60_000).contains(&interval) {
                return Err(AppError::validation("轮询间隔需要在 200 到 60000 毫秒之间"));
            }
        }
        if poll.body.as_ref().is_some_and(Value::is_null) {
            poll.body = None;
        }
        if poll.body.as_ref().is_some_and(|body| !body.is_object()) {
            return Err(AppError::validation("轮询正文必须是对象"));
        }
        if let Some(body) = &poll.body {
            validate_template_secrets(body)?;
        }
    }
    if recipe.task_id_path.is_some() != recipe.poll.is_some() {
        return Err(AppError::validation("异步视频需要任务编号路径和轮询配置"));
    }
    if recipe.result_kind.as_deref() == Some("binary")
        && (recipe.task_id_path.is_some() || recipe.poll.is_some())
    {
        return Err(AppError::validation("二进制结果不能轮询"));
    }
    Ok(())
}

fn validate_template_secrets(value: &Value) -> Result<(), AppError> {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                if crate::redact::is_sensitive_key(key) && item.as_str() != Some("{{apiKey}}") {
                    return Err(AppError::validation(format!(
                        "敏感 HTTP 字段「{key}」必须使用 {{apiKey}}"
                    )));
                }
                validate_template_secrets(item)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                validate_template_secrets(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn require_json_path(path: &str, label: &str) -> Result<(), AppError> {
    if path.is_empty() || path.len() > 120 || !path.starts_with('$') || !json_path_syntax(path) {
        return Err(AppError::validation(format!("{label} 必须是 JSON 路径")));
    }
    Ok(())
}

fn json_path_syntax(path: &str) -> bool {
    let mut rest = &path[1..];
    if rest.is_empty() {
        return true;
    }
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('.') {
            let end = stripped.find(['.', '[']).unwrap_or(stripped.len());
            let name = &stripped[..end];
            if name.is_empty()
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
            {
                return false;
            }
            rest = &stripped[end..];
        } else if let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else {
                return false;
            };
            if stripped[..end].is_empty() || !stripped[..end].chars().all(|ch| ch.is_ascii_digit())
            {
                return false;
            }
            rest = &stripped[end + 1..];
        } else {
            return false;
        }
    }
    true
}

fn clean_path(path: Option<String>, label: &str) -> Result<Option<String>, AppError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path.trim().trim_start_matches('/').to_string();
    if path.is_empty() {
        return Ok(None);
    }
    validate_rel_path(&path, label)?;
    Ok(Some(path))
}

fn validate_value(definition: &ParameterDefinition, value: &Value) -> Result<(), AppError> {
    match definition.kind {
        ParameterType::Boolean => {
            if !value.is_boolean() {
                return Err(type_error(definition, "布尔值"));
            }
        }
        ParameterType::Number | ParameterType::Range => {
            let number = value
                .as_f64()
                .ok_or_else(|| type_error(definition, "数字"))?;
            if let Some(min) = definition.min {
                if number < min {
                    return Err(AppError::validation(format!(
                        "参数「{}」不能小于 {min}",
                        definition.label
                    )));
                }
            }
            if let Some(max) = definition.max {
                if number > max {
                    return Err(AppError::validation(format!(
                        "参数「{}」不能大于 {max}",
                        definition.label
                    )));
                }
            }
        }
        ParameterType::Text | ParameterType::Textarea => {
            let text = value
                .as_str()
                .ok_or_else(|| type_error(definition, "文本"))?;
            if text.chars().count() > 100_000 {
                return Err(AppError::validation(format!(
                    "参数「{}」过长",
                    definition.label
                )));
            }
        }
        ParameterType::Select => {
            let options = definition.options.as_deref().unwrap_or(&[]);
            if !options
                .iter()
                .any(|option| same_value(&option.value, value))
            {
                return Err(AppError::validation(format!(
                    "参数「{}」的值不在选项中",
                    definition.label
                )));
            }
        }
        ParameterType::Image | ParameterType::Video | ParameterType::Audio => {
            validate_path_value(definition, value)?;
        }
        ParameterType::Images | ParameterType::Videos | ParameterType::Audios => {
            let items = value
                .as_array()
                .ok_or_else(|| type_error(definition, "文件列表"))?;
            if items.len() > 16 {
                return Err(AppError::validation(format!(
                    "参数「{}」最多 16 个文件",
                    definition.label
                )));
            }
            for item in items {
                validate_path_value(definition, item)?;
            }
        }
    }
    Ok(())
}

fn validate_path_value(definition: &ParameterDefinition, value: &Value) -> Result<(), AppError> {
    let path = value
        .as_str()
        .ok_or_else(|| type_error(definition, "文件路径"))?;
    if path.trim().is_empty() || path.contains('\n') || path.len() > 1000 {
        return Err(AppError::validation(format!(
            "参数「{}」的文件路径无效",
            definition.label
        )));
    }
    Ok(())
}

fn type_error(definition: &ParameterDefinition, expected: &str) -> AppError {
    AppError::validation(format!("参数「{}」需要{expected}", definition.label))
}

pub fn is_empty_value(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        Some(Value::Array(items)) if items.is_empty() => true,
        Some(Value::Object(map)) if map.is_empty() => true,
        _ => false,
    }
}

fn same_value(left: &Value, right: &Value) -> bool {
    left == right
        || left
            .as_f64()
            .is_some_and(|first| right.as_f64() == Some(first))
}

fn is_key(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
}

pub fn validate_and_migrate_schema_version(
    version: u32,
    target_name: &str,
) -> Result<u32, AppError> {
    if version == 0 {
        return Err(AppError::validation(format!(
            "{target_name} schemaVersion 不能为 0"
        )));
    }
    // 当前版本为 1。未来若有 v2/v3，可在此增加自动迁移逻辑
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ConstraintDefinition, HeaderTemplate, HttpRecipe, ParameterType, SelectOption,
        WorkflowBinding,
    };
    use serde_json::json;

    fn temperature() -> ParameterDefinition {
        ParameterDefinition {
            key: "temperature".to_string(),
            label: "Temperature".to_string(),
            kind: ParameterType::Range,
            default: Some(json!(0.7)),
            min: Some(0.0),
            max: Some(2.0),
            step: Some(0.1),
            options: None,
            required: None,
            advanced: None,
            group: None,
        }
    }

    #[test]
    fn applies_defaults_and_rejects_conflicts() {
        let definitions = vec![
            temperature(),
            ParameterDefinition {
                key: "seed".to_string(),
                label: "Seed".to_string(),
                kind: ParameterType::Number,
                default: None,
                min: Some(0.0),
                max: None,
                step: Some(1.0),
                options: None,
                required: None,
                advanced: Some(true),
                group: None,
            },
            ParameterDefinition {
                key: "stop".to_string(),
                label: "Stop".to_string(),
                kind: ParameterType::Text,
                default: None,
                min: None,
                max: None,
                step: None,
                options: None,
                required: None,
                advanced: Some(true),
                group: None,
            },
        ];
        let constraints = vec![ConstraintDefinition::MutuallyExclusive {
            fields: vec!["seed".to_string(), "stop".to_string()],
        }];
        let values = normalize_parameters(&definitions, &Map::new(), &constraints).unwrap();
        assert_eq!(values.get("temperature"), Some(&json!(0.7)));
        assert!(values.get("seed").is_none());

        let mut input = Map::new();
        input.insert("seed".to_string(), json!(1));
        input.insert("stop".to_string(), json!("END"));
        let error = normalize_parameters(&definitions, &input, &constraints).unwrap_err();
        assert_eq!(error.code, crate::error::VALIDATION);

        input.remove("stop");
        input.insert("temperature".to_string(), json!(9));
        assert!(normalize_parameters(&definitions, &input, &constraints).is_err());

        let mut unknown = Map::new();
        unknown.insert("nope".to_string(), json!(1));
        assert!(normalize_parameters(&definitions, &unknown, &constraints).is_err());
    }

    #[test]
    fn select_accepts_numeric_equivalent_and_rejects_unknown() {
        let definitions = vec![ParameterDefinition {
            key: "quality".to_string(),
            label: "Quality".to_string(),
            kind: ParameterType::Select,
            default: None,
            min: None,
            max: None,
            step: None,
            options: Some(vec![SelectOption {
                label: "High".to_string(),
                value: json!(1),
            }]),
            required: Some(true),
            advanced: None,
            group: None,
        }];
        let mut input = Map::new();
        input.insert("quality".to_string(), json!(1.0));
        assert!(normalize_parameters(&definitions, &input, &[]).is_ok());
        input.insert("quality".to_string(), json!("high"));
        assert!(normalize_parameters(&definitions, &input, &[]).is_err());
    }

    #[test]
    fn keeps_workflow_when_paths_are_empty() {
        let mut transport = Some(TransportDefinition {
            chat_path: None,
            models_path: None,
            image_path: None,
            image_edit_path: None,
            speech_path: None,
            http: None,
            workflow: Some(json!({ "6": { "inputs": { "text": "keep" } } })),
            bindings: Some(vec![WorkflowBinding {
                key: " prompt ".to_string(),
                node: " 6 ".to_string(),
                input: " text ".to_string(),
            }]),
        });
        validate_definition(&mut [], &mut [], &mut transport, &json!({})).unwrap();
        let saved = transport.expect("workflow transport");
        assert!(saved.workflow.is_some());
        let binding = &saved.bindings.expect("bindings")[0];
        assert_eq!(binding.key, "prompt");
        assert_eq!(binding.node, "6");
        assert_eq!(binding.input, "text");

        let mut broken = Some(TransportDefinition {
            workflow: Some(json!(["nope"])),
            ..TransportDefinition::default()
        });
        assert!(validate_definition(&mut [], &mut [], &mut broken, &json!({})).is_err());
    }

    #[test]
    fn requires_constraint_enforces_dependencies() {
        let definitions = vec![
            ParameterDefinition {
                key: "denoise".to_string(),
                label: "Denoise".to_string(),
                kind: ParameterType::Range,
                default: None,
                min: Some(0.0),
                max: Some(1.0),
                step: Some(0.05),
                options: None,
                required: None,
                advanced: None,
                group: Some("sampler".to_string()),
            },
            ParameterDefinition {
                key: "sampler_name".to_string(),
                label: "Sampler".to_string(),
                kind: ParameterType::Text,
                default: None,
                min: None,
                max: None,
                step: None,
                options: None,
                required: None,
                advanced: None,
                group: Some("sampler".to_string()),
            },
        ];
        let constraints = vec![ConstraintDefinition::Requires {
            field: "denoise".to_string(),
            required_fields: vec!["sampler_name".to_string()],
        }];

        // denoise 未提供时，不需要 sampler_name
        let mut input = Map::new();
        assert!(normalize_parameters(&definitions, &input, &constraints).is_ok());

        // denoise 提供了，但没有 sampler_name 时报错
        input.insert("denoise".to_string(), json!(0.75));
        let err = normalize_parameters(&definitions, &input, &constraints).unwrap_err();
        assert!(err.to_string().contains("需要同时提供"));

        // 两者都提供时通过
        input.insert("sampler_name".to_string(), json!("euler_a"));
        assert!(normalize_parameters(&definitions, &input, &constraints).is_ok());
    }

    #[test]
    fn conditional_visibility_hides_and_skips_required() {
        let definitions = vec![
            ParameterDefinition {
                key: "mode".to_string(),
                label: "Mode".to_string(),
                kind: ParameterType::Text,
                default: Some(json!("simple")),
                min: None,
                max: None,
                step: None,
                options: None,
                required: Some(true),
                advanced: None,
                group: None,
            },
            ParameterDefinition {
                key: "expert_seed".to_string(),
                label: "Expert Seed".to_string(),
                kind: ParameterType::Number,
                default: None,
                min: None,
                max: None,
                step: None,
                options: None,
                required: Some(true), // 必填，但条件隐藏时不应报错
                advanced: Some(true),
                group: Some("expert".to_string()),
            },
        ];
        let constraints = vec![ConstraintDefinition::ConditionalVisibility {
            field: "expert_seed".to_string(),
            depends_on: "mode".to_string(),
            equals: Some(json!("expert")),
            not_equals: None,
        }];

        // 默认 mode 是 simple，expert_seed 隐藏，不报必填错，也不在结果中
        let input = Map::new();
        let res = normalize_parameters(&definitions, &input, &constraints).unwrap();
        assert_eq!(res.get("mode"), Some(&json!("simple")));
        assert!(!res.contains_key("expert_seed"));

        // 当 mode 为 expert 时，expert_seed 显示且必填，此时缺少则报错
        let mut expert_input = Map::new();
        expert_input.insert("mode".to_string(), json!("expert"));
        let err = normalize_parameters(&definitions, &expert_input, &constraints).unwrap_err();
        assert!(err.to_string().contains("必填项"));

        // 填入后通过
        expert_input.insert("expert_seed".to_string(), json!(42));
        let res2 = normalize_parameters(&definitions, &expert_input, &constraints).unwrap();
        assert_eq!(res2.get("expert_seed"), Some(&json!(42)));
    }

    #[test]
    fn test_schema_version_validation() {
        assert!(validate_and_migrate_schema_version(1, "Model").is_ok());
        assert_eq!(validate_and_migrate_schema_version(1, "Model").unwrap(), 1);
        assert!(validate_and_migrate_schema_version(0, "Model").is_err());
    }

    #[test]
    fn sensitive_headers_accept_bearer_api_key_placeholder_only() {
        let recipe = |value: &str| HttpRecipe {
            method: "POST".to_string(),
            path: "v1/test".to_string(),
            headers: vec![HeaderTemplate {
                name: "Authorization".to_string(),
                value: value.to_string(),
            }],
            body: None,
            task_id_path: None,
            poll: None,
            result_path: "$".to_string(),
            result_kind: None,
        };

        let mut bearer = TransportDefinition {
            http: Some(recipe("Bearer {{apiKey}}")),
            ..TransportDefinition::default()
        };
        assert!(validate_http(&mut bearer).is_ok());

        let mut embedded_literal = TransportDefinition {
            http: Some(recipe("Bearer sk-secret-value")),
            ..TransportDefinition::default()
        };
        assert!(validate_http(&mut embedded_literal).is_err());
    }

    #[test]
    fn test_fuzz_extreme_json_and_types() {
        let definitions = vec![
            ParameterDefinition {
                key: "text_param".to_string(),
                label: "文本".to_string(),
                kind: ParameterType::Text,
                default: None,
                min: None,
                max: None,
                step: None,
                options: None,
                required: Some(true),
                advanced: None,
                group: None,
            },
            ParameterDefinition {
                key: "num_param".to_string(),
                label: "数字".to_string(),
                kind: ParameterType::Number,
                default: None,
                min: Some(0.0),
                max: Some(100.0),
                step: None,
                options: None,
                required: Some(true),
                advanced: None,
                group: None,
            },
        ];

        // 1. 极限大文本（50KB）作为合法大输入
        let mut map = Map::new();
        let large_str = "A".repeat(50_000);
        map.insert("text_param".to_string(), json!(large_str));
        map.insert("num_param".to_string(), json!(50.0));
        let res = normalize_parameters(&definitions, &map, &[]);
        assert!(res.is_ok());

        // 1.1 超长字符串（120KB+，超过 100_000 上限）应被安全边界拦截，防御 DoS
        let mut oversized_map = Map::new();
        let massive_str = "A".repeat(120_000);
        oversized_map.insert("text_param".to_string(), json!(massive_str));
        oversized_map.insert("num_param".to_string(), json!(50.0));
        let oversized_res = normalize_parameters(&definitions, &oversized_map, &[]);
        assert!(oversized_res.is_err());
        assert!(oversized_res.unwrap_err().to_string().contains("过长"));

        // 2. 错误类型输入（给 Number 传入 Object/Array/String）
        let mut wrong_type_map = Map::new();
        wrong_type_map.insert("text_param".to_string(), json!("valid text"));
        wrong_type_map.insert("num_param".to_string(), json!({"nested": "invalid"}));
        assert!(normalize_parameters(&definitions, &wrong_type_map, &[]).is_err());

        // 3. 越界数值极限
        let mut out_of_bounds = Map::new();
        out_of_bounds.insert("text_param".to_string(), json!("valid"));
        out_of_bounds.insert("num_param".to_string(), json!(999999.0));
        assert!(normalize_parameters(&definitions, &out_of_bounds, &[]).is_err());

        out_of_bounds.insert("num_param".to_string(), json!(-1.0));
        assert!(normalize_parameters(&definitions, &out_of_bounds, &[]).is_err());
    }

    #[test]
    fn test_fuzz_unicode_and_special_characters() {
        let definitions = vec![ParameterDefinition {
            key: "special_input".to_string(),
            label: "特殊输入".to_string(),
            kind: ParameterType::Text,
            default: None,
            min: None,
            max: None,
            step: None,
            options: None,
            required: Some(true),
            advanced: None,
            group: None,
        }];

        // 测试各种复杂 Unicode、Emoji、多音字、零宽字符、路径穿越与换行符
        let test_cases = vec![
            "👾🚀🔥✨🎯🌈💖🎉",
            "你好，世界！这是一段中文测试、繁體中文、日本語、한국어、العربية",
            "../../../../../../etc/passwd",
            "..\\..\\..\\Windows\\System32\\cmd.exe",
            "\"'; DROP TABLE models; -- <script>alert(1)</script>",
            "\u{200B}\u{200C}\u{200D}\u{FEFF}", // 零宽字符
            "\n\r\t\0\x1F",                     // 控制字符
        ];

        for tc in test_cases {
            let mut map = Map::new();
            map.insert("special_input".to_string(), json!(tc));
            let res = normalize_parameters(&definitions, &map, &[]);
            assert!(res.is_ok(), "Failed on test case: {:?}", tc);
            assert_eq!(
                res.unwrap().get("special_input").and_then(|v| v.as_str()),
                Some(tc)
            );
        }
    }
}
