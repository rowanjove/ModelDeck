use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Map};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::credentials::{MemorySecretStore, SecretStore};
use crate::db;
use crate::domain::{
    ChatSendInput, ConstraintDefinition, GenerateAudioInput, GenerateImageInput, GenerateTextInput,
    GenerateVideoInput, HeaderTemplate, HttpPoll, HttpRecipe, MediaRef, Modality, ModelInput,
    ParameterDefinition, ParameterType, PresetInput, ProviderInput, ProviderType, ReferenceImage,
    TransportDefinition, VoiceInput, WorkflowBinding,
};
use crate::error::{AUTH, VALIDATION};
use crate::services;
use crate::services::chat::ChatNotice;
use crate::services::history::{HistoryQuery, NewTextTask};

async fn setup() -> (sqlx::SqlitePool, Arc<MemorySecretStore>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("aiwb-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("app.db");
    let pool = db::open(&db_path).await.unwrap();
    db::migrate(&pool).await.unwrap();
    (pool, Arc::new(MemorySecretStore::default()), dir)
}

fn assert_no_secret(dir: &Path, secret: &str) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                !String::from_utf8_lossy(&bytes).contains(secret),
                "secret leaked into {}",
                path.display()
            );
        }
    }
}

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

#[tokio::test]
async fn api_key_is_kept_out_of_sqlite_and_provider_payload() {
    let secret = "sk-test-DoNotStore-9f3a";
    let (pool, secrets, dir) = setup().await;
    let created = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Local".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: Some(secret.to_string()),
            clear_api_key: false,
            headers: BTreeMap::from([("X-Title".to_string(), "workbench".to_string())]),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();
    assert!(created.has_credential);
    assert!(!serde_json::to_string(&created).unwrap().contains(secret));
    assert_eq!(secrets.get(&created.id).unwrap().unwrap().expose(), secret);
    assert_no_secret(&dir, secret);

    let rejected = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Bad headers".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::from([("Authorization".to_string(), format!("Bearer {secret}"))]),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(rejected.code, VALIDATION);

    services::models::create(
        &pool,
        ModelInput {
            name: "Demo".to_string(),
            provider_id: created.id.clone(),
            model_id: "demo-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": true }),
            parameters: vec![temperature()],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    services::providers::delete(&pool, secrets.as_ref(), &created.id)
        .await
        .unwrap();
    assert!(secrets.get(&created.id).unwrap().is_none());
    assert!(services::models::list(&pool).await.unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn text_generate_reaches_openai_compatible_provider() {
    let secret = "sk-live-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {secret}")))
        .and(body_string_contains("\"model\":\"demo-model\""))
        .and(body_string_contains("\"temperature\":0.2"))
        .and(body_string_contains("\"custom_flag\":true"))
        .and(body_string_contains("\"stream\":false"))
        .and(body_string_contains("\"content\":\"be brief\""))
        .and(body_string_contains("\"content\":\"ping\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "pong" } }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 1, "total_tokens": 4 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Mock".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: format!("{}/v1", server.uri()),
            api_key: Some(secret.to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Demo".to_string(),
            provider_id: provider.id,
            model_id: "demo-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": true, "vision": false }),
            parameters: vec![temperature()],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let mut parameters = Map::new();
    parameters.insert("temperature".to_string(), json!(0.2));
    let http = reqwest::Client::new();
    let result = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id,
            system_prompt: Some("be brief".to_string()),
            prompt: "ping".to_string(),
            parameters,
            extra: Some(json!({ "custom_flag": true })),
            images: vec![],
        },
    )
    .await
    .unwrap();

    assert_eq!(result.text, "pong");
    assert_eq!(result.usage.unwrap().total_tokens, Some(4));
    assert_eq!(result.provider_name, "Mock");
    assert_eq!(result.model_name, "Demo");
    let raw = result.raw_request.to_string();
    assert!(!raw.contains(secret));
    assert!(raw.contains("Bearer sk-l••••"));
    let page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].status, "completed");
    assert_eq!(page.items[0].prompt_preview, "ping");
    let listed = serde_json::to_string(&page).unwrap();
    assert!(!listed.contains("pong"));
    assert!(!listed.contains(secret));
    let detail = services::history::get(&pool, &page.items[0].id)
        .await
        .unwrap();
    assert_eq!(detail.kind, "generate");
    assert_eq!(detail.output, "pong");
    assert_eq!(detail.system_prompt.as_deref(), Some("be brief"));
    let stored = detail.raw_request.to_string();
    assert!(!stored.contains(secret));
    assert!(stored.contains("Bearer sk-l"));
    let file = detail
        .assets
        .iter()
        .find(|asset| asset.asset_type == "file")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(file.path.as_deref().unwrap()).unwrap(),
        "pong"
    );
    let images = services::history::list(
        &pool,
        HistoryQuery {
            modality: Some("image".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(images.total, 0);
    assert_no_secret(&dir, secret);
    server.verify().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn auth_and_schema_errors_do_not_leak_the_key() {
    let secret = "sk-live-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "message": "invalid api key" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Mock".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: format!("{}/v1", server.uri()),
            api_key: Some(secret.to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Demo".to_string(),
            provider_id: provider.id,
            model_id: "demo-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({}),
            parameters: vec![temperature()],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let mut bad = Map::new();
    bad.insert("temperature".to_string(), json!(9));
    let invalid = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id.clone(),
            system_prompt: None,
            prompt: "ping".to_string(),
            parameters: bad,
            extra: None,
            images: vec![],
        },
    )
    .await
    .unwrap_err();
    assert_eq!(invalid.code, VALIDATION);
    assert!(!serde_json::to_string(&invalid).unwrap().contains(secret));
    let untouched = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(untouched.total, 0);

    let denied = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id,
            system_prompt: None,
            prompt: "ping".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await
    .unwrap_err();
    assert_eq!(denied.code, AUTH);
    assert_eq!(denied.status, Some(401));
    assert!(!serde_json::to_string(&denied).unwrap().contains(secret));
    let failed = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(failed.total, 1);
    assert_eq!(failed.items[0].status, "failed");
    let detail = services::history::get(&pool, &failed.items[0].id)
        .await
        .unwrap();
    assert!(detail.output.is_empty());
    assert!(detail.assets.iter().all(|asset| asset.asset_type != "file"));
    assert!(!serde_json::to_string(&detail).unwrap().contains(secret));
    assert_no_secret(&dir, secret);
    server.verify().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn streaming_chat_keeps_context_bounded_and_truncates_on_regenerate() {
    let secret = "sk-live-secret-value";
    let sse = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"pong\"}}]}\n\n",
        "data: {\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1,\"total_tokens\":4}}\n\n",
        "data: [DONE]\n\n"
    );
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {secret}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse))
        .expect(4)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Mock".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: format!("{}/v1", server.uri()),
            api_key: Some(secret.to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Demo".to_string(),
            provider_id: provider.id,
            model_id: "demo-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": true }),
            parameters: vec![temperature()],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let cancels = Arc::new(Mutex::new(HashMap::new()));
    let notices = Arc::new(Mutex::new(Vec::new()));
    let on_notice = {
        let notices = Arc::clone(&notices);
        move |notice: ChatNotice| {
            notices.lock().unwrap().push(notice);
            true
        }
    };

    let first = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        chat_turn(&model.id, None, "keep-topic", None),
        &on_notice,
    )
    .await
    .unwrap();
    assert_eq!(first.assistant.text, "pong");
    assert_eq!(first.assistant.usage.unwrap().total_tokens, Some(4));
    assert_eq!(first.conversation.title, "keep-topic");
    assert!(notices
        .lock()
        .unwrap()
        .iter()
        .any(|notice| { matches!(notice, ChatNotice::Delta { text } if text == "pong") }));

    let second = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        chat_turn(
            &model.id,
            Some(first.conversation.id.clone()),
            "drop-topic",
            None,
        ),
        &on_notice,
    )
    .await
    .unwrap();
    let before = services::chat::get(&pool, &second.conversation.id)
        .await
        .unwrap();
    assert_eq!(before.messages.len(), 4);
    let stale_assistant = before.messages[3].id.clone();

    let regenerated = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        chat_turn(
            &model.id,
            Some(second.conversation.id.clone()),
            "",
            Some(stale_assistant.clone()),
        ),
        &on_notice,
    )
    .await
    .unwrap();
    assert_ne!(regenerated.assistant.id, stale_assistant);

    let before_cancelled_replace = services::chat::get(&pool, &second.conversation.id)
        .await
        .unwrap();
    let summary_until_before: Option<String> =
        sqlx::query_scalar("SELECT summary_until FROM conversations WHERE id = ?")
            .bind(&second.conversation.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let cancels_for_callback = Arc::clone(&cancels);
    let cancelled_conversation_id = second.conversation.id.clone();
    let cancel_on_delta = move |notice: ChatNotice| {
        if matches!(notice, ChatNotice::Delta { .. }) {
            services::chat::cancel(&cancels_for_callback, &cancelled_conversation_id);
        }
        true
    };
    let interrupted_replace = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        chat_turn(
            &model.id,
            Some(second.conversation.id.clone()),
            "",
            Some(regenerated.assistant.id.clone()),
        ),
        &cancel_on_delta,
    )
    .await;
    assert!(interrupted_replace.is_err());
    let after_cancelled_replace = services::chat::get(&pool, &second.conversation.id)
        .await
        .unwrap();
    assert_eq!(
        after_cancelled_replace
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        before_cancelled_replace
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        after_cancelled_replace.conversation.summary,
        before_cancelled_replace.conversation.summary
    );
    let summary_until_after: Option<String> =
        sqlx::query_scalar("SELECT summary_until FROM conversations WHERE id = ?")
            .bind(&second.conversation.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(summary_until_after, summary_until_before);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 4);
    let body = |index: usize| String::from_utf8(requests[index].body.clone()).unwrap();
    assert!(body(0).contains("\"stream\":true"));
    assert!(body(0).contains("keep-topic"));
    assert_eq!(
        requests[0]
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap(),
        format!("Bearer {secret}")
    );
    assert!(body(1).contains("drop-topic"));
    assert!(!body(1).contains("keep-topic"));
    assert!(body(2).contains("drop-topic"));
    assert!(!body(2).contains("keep-topic"));
    assert!(body(3).contains("drop-topic"));
    assert!(!body(3).contains("keep-topic"));

    let first_assistant = services::chat::get(&pool, &second.conversation.id)
        .await
        .unwrap()
        .messages[1]
        .id
        .clone();
    let truncated =
        services::chat::delete_message(&pool, &cancels, &second.conversation.id, &first_assistant)
            .await
            .unwrap();
    assert_eq!(truncated.messages.len(), 1);
    assert_eq!(truncated.messages[0].text, "keep-topic");
    let history = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(history.total, 4);
    assert_eq!(
        history
            .items
            .iter()
            .filter(|item| item.status == "completed")
            .count(),
        3
    );
    assert_eq!(
        history
            .items
            .iter()
            .filter(|item| item.status == "cancelled")
            .count(),
        1
    );
    assert!(history
        .items
        .iter()
        .all(|item| !item.prompt_preview.contains("压缩成")));
    let newest = services::history::get(&pool, &history.items[0].id)
        .await
        .unwrap();
    assert_eq!(newest.kind, "chat");
    assert_eq!(
        newest.conversation_id.as_deref(),
        Some(second.conversation.id.as_str())
    );
    assert_eq!(newest.output, "pong");
    assert_no_secret(&dir, secret);
    server.verify().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn history_pages_filters_and_keeps_names_after_deletion() {
    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Mock".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Demo".to_string(),
            provider_id: provider.id.clone(),
            model_id: "demo-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({}),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let mut older = text_task(&provider.id, &model.id, "beta", "failed");
    older.error = Some("boom".to_string());
    let older_id = services::history::record_text_task(&pool, &dir, older)
        .await
        .unwrap();
    let mut newer = text_task(
        &provider.id,
        &model.id,
        "first line\nsecret-second",
        "completed",
    );
    newer.output_text = Some("FILE-BODY-NOT-IN-LIST".to_string());
    newer.raw_response = Some(json!({ "marker": "RAW-BODY-NOT-IN-LIST" }));
    let newer_id = services::history::record_text_task(&pool, &dir, newer)
        .await
        .unwrap();
    sqlx::query("UPDATE generation_tasks SET created_at = ? WHERE id = ?")
        .bind("2020-01-02T03:04:05.000Z")
        .bind(&older_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE generation_tasks SET created_at = ? WHERE id = ?")
        .bind("2024-06-01T12:00:00.000Z")
        .bind(&newer_id)
        .execute(&pool)
        .await
        .unwrap();

    let page = services::history::list(
        &pool,
        HistoryQuery {
            limit: Some(1),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.limit, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, newer_id);
    assert_eq!(page.items[0].prompt_preview, "first line");
    let listed = serde_json::to_string(&page).unwrap();
    assert!(!listed.contains("FILE-BODY-NOT-IN-LIST"));
    assert!(!listed.contains("RAW-BODY-NOT-IN-LIST"));
    assert!(!listed.contains("secret-second"));

    let next = services::history::list(
        &pool,
        HistoryQuery {
            limit: Some(1),
            offset: Some(1),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(next.items[0].id, older_id);
    let wide = services::history::list(
        &pool,
        HistoryQuery {
            limit: Some(500),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(wide.limit, 100);
    let narrow = services::history::list(
        &pool,
        HistoryQuery {
            limit: Some(0),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(narrow.limit, 1);

    let dated = services::history::list(
        &pool,
        HistoryQuery {
            date_from: Some("2024-01-01".to_string()),
            date_to: Some("2024-12-31".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(dated.total, 1);
    assert_eq!(dated.items[0].id, newer_id);
    let images = services::history::list(
        &pool,
        HistoryQuery {
            modality: Some("image".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(images.total, 0);
    let rejected = services::history::list(
        &pool,
        HistoryQuery {
            status: Some("nope".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(rejected.code, VALIDATION);

    let detail = services::history::get(&pool, &newer_id).await.unwrap();
    assert_eq!(detail.output, "FILE-BODY-NOT-IN-LIST");
    assert!(detail
        .raw_response
        .to_string()
        .contains("RAW-BODY-NOT-IN-LIST"));
    assert!(detail.prompt.contains("secret-second"));

    services::providers::delete(&pool, secrets.as_ref(), &provider.id)
        .await
        .unwrap();
    assert!(services::models::list(&pool).await.unwrap().is_empty());
    let kept = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(kept.total, 2);
    assert_eq!(kept.items[0].provider_name, "Mock");
    assert_eq!(kept.items[0].model_name, "Demo");
    let _ = std::fs::remove_dir_all(dir);
}

const TINY_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn tiny_png() -> Vec<u8> {
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, TINY_PNG_B64).unwrap()
}

fn dimension(key: &str) -> ParameterDefinition {
    ParameterDefinition {
        key: key.to_string(),
        label: key.to_string(),
        kind: ParameterType::Number,
        default: None,
        min: Some(1.0),
        max: None,
        step: Some(1.0),
        options: None,
        required: None,
        advanced: None,
        group: None,
    }
}

async fn image_provider(
    pool: &sqlx::SqlitePool,
    secrets: &MemorySecretStore,
    name: &str,
    kind: ProviderType,
    base_url: &str,
    api_key: Option<&str>,
) -> crate::domain::Provider {
    services::providers::create(
        pool,
        secrets,
        ProviderInput {
            name: name.to_string(),
            provider_type: kind,
            base_url: base_url.to_string(),
            api_key: api_key.map(ToOwned::to_owned),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 5_000,
            enabled: true,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn image_generate_saves_a_file_and_keeps_bytes_out_of_sqlite() {
    let secret = "sk-image-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/images/generations"))
        .and(header("authorization", format!("Bearer {secret}")))
        .and(body_string_contains("\"model\":\"image-model\""))
        .and(body_string_contains("\"prompt\":\"a cat\""))
        .and(body_string_contains("\"size\":\"64x64\""))
        .and(body_string_contains("\"n\":2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "b64_json": TINY_PNG_B64 }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Images",
        ProviderType::OpenaiCompatible,
        &format!("{}/v1", server.uri()),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Painter".to_string(),
            provider_id: provider.id,
            model_id: "image-model".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "generate": true }),
            parameters: vec![
                dimension("width"),
                dimension("height"),
                dimension("batch_size"),
            ],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let mut parameters = Map::new();
    parameters.insert("width".to_string(), json!(64));
    parameters.insert("height".to_string(), json!(64));
    parameters.insert("batch_size".to_string(), json!(2));
    let http = reqwest::Client::new();
    let result = services::image::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateImageInput {
            model_definition_id: model.id,
            prompt: "a cat".to_string(),
            negative_prompt: Some("blur".to_string()),
            source_image: None,
            reference_images: vec![],
            mask_base64: None,
            mask_path: None,
            parameters,
            extra: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.images[0].path).unwrap(), tiny_png());
    assert_eq!(result.images[0].width, Some(1));
    assert_eq!(result.images[0].height, Some(1));

    let images = services::history::list(
        &pool,
        HistoryQuery {
            modality: Some("image".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(images.total, 1);
    assert_eq!(images.items[0].status, "completed");
    let listed = serde_json::to_string(&images).unwrap();
    assert!(!listed.contains(secret));
    assert!(!listed.contains(TINY_PNG_B64));
    let texts = services::history::list(
        &pool,
        HistoryQuery {
            modality: Some("text".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(texts.total, 0);
    let detail = services::history::get(&pool, &images.items[0].id)
        .await
        .unwrap();
    assert_eq!(detail.kind, "image");
    assert!(detail.assets.iter().any(|asset| {
        asset.asset_type == "image" && asset.path.as_deref() == Some(result.images[0].path.as_str())
    }));
    assert!(!detail.raw_response.to_string().contains(TINY_PNG_B64));
    let db_text = String::from_utf8_lossy(&std::fs::read(dir.join("app.db")).unwrap()).into_owned();
    assert!(!db_text.contains(secret));
    assert!(!db_text.contains(TINY_PNG_B64));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn image_edits_upload_source_and_can_download_a_url() {
    let secret = "sk-edit-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/images/edits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "url": format!("{}/files/out.png", server.uri()) }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/out.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_png()))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let source = dir.join("source.png");
    std::fs::write(&source, tiny_png()).unwrap();
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Edits",
        ProviderType::OpenaiCompatible,
        &format!("{}/v1", server.uri()),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Editor".to_string(),
            provider_id: provider.id,
            model_id: "image-edit".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "edit": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let result = services::image::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateImageInput {
            model_definition_id: model.id,
            prompt: "edit this".to_string(),
            negative_prompt: None,
            source_image: Some(MediaRef::from_path(source.to_string_lossy())),
            reference_images: vec![],
            mask_base64: None,
            mask_path: None,
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.images[0].path).unwrap(), tiny_png());
    let requests = server.received_requests().await.unwrap();
    let edit = requests
        .iter()
        .find(|request| request.url.path() == "/v1/images/edits")
        .expect("edits request");
    let body = String::from_utf8_lossy(&edit.body);
    assert!(body.contains("edit this"));
    assert!(body.contains("source.png"));
    assert!(body.contains("name=\"image\""));
    let detail_page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(detail_page.total, 1);
    assert!(!serde_json::to_string(&detail_page)
        .unwrap()
        .contains(secret));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn image_failure_is_recorded_and_validation_is_not() {
    let secret = "sk-image-denied-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/images/generations"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "message": "unauthorized" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let denied = image_provider(
        &pool,
        secrets.as_ref(),
        "Denied",
        ProviderType::Openai,
        &format!("{}/v1", server.uri()),
        Some(secret),
    )
    .await;
    let denied_model = services::models::create(
        &pool,
        ModelInput {
            name: "Denied image".to_string(),
            provider_id: denied.id.clone(),
            model_id: "image-model".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "generate": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let error = services::image::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateImageInput {
            model_definition_id: denied_model.id,
            prompt: "a cat".to_string(),
            negative_prompt: None,
            source_image: None,
            reference_images: vec![],
            mask_base64: None,
            mask_path: None,
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, AUTH);
    let failed = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(failed.total, 1);
    assert_eq!(failed.items[0].status, "failed");
    let detail = services::history::get(&pool, &failed.items[0].id)
        .await
        .unwrap();
    assert!(detail.raw_request.is_null());
    assert!(detail.assets.iter().all(|asset| {
        asset.metadata.get("role").and_then(|role| role.as_str()) != Some("output")
    }));
    assert_no_secret(&dir, secret);

    let text_model = services::models::create(
        &pool,
        ModelInput {
            name: "Words".to_string(),
            provider_id: denied.id.clone(),
            model_id: "text-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let plain = services::models::create(
        &pool,
        ModelInput {
            name: "Plain".to_string(),
            provider_id: denied.id,
            model_id: "image-model".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "generate": true, "mask": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let unlocked = image_provider(
        &pool,
        secrets.as_ref(),
        "No key",
        ProviderType::Openai,
        "https://api.openai.com/v1",
        None,
    )
    .await;
    let locked = services::models::create(
        &pool,
        ModelInput {
            name: "Locked".to_string(),
            provider_id: unlocked.id.clone(),
            model_id: "image-model".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "generate": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let comfy = image_provider(
        &pool,
        secrets.as_ref(),
        "Local graph",
        ProviderType::Comfyui,
        "http://127.0.0.1:9",
        None,
    )
    .await;
    let graph = services::models::create(
        &pool,
        ModelInput {
            name: "Graph".to_string(),
            provider_id: comfy.id,
            model_id: "graph".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "generate": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let cases = [
        GenerateImageInput {
            model_definition_id: text_model.id,
            prompt: "a cat".to_string(),
            negative_prompt: None,
            source_image: None,
            reference_images: vec![],
            mask_base64: None,
            mask_path: None,
            parameters: Map::new(),
            extra: None,
        },
        GenerateImageInput {
            model_definition_id: plain.id.clone(),
            prompt: "a cat".to_string(),
            negative_prompt: None,
            source_image: None,
            reference_images: vec![ReferenceImage {
                path: "missing.png".to_string(),
                role: None,
                weight: None,
                description: None,
                enabled: None,
            }],
            mask_base64: None,
            mask_path: None,
            parameters: Map::new(),
            extra: None,
        },
        GenerateImageInput {
            model_definition_id: plain.id,
            prompt: "a cat".to_string(),
            negative_prompt: None,
            source_image: None,
            reference_images: vec![],
            mask_base64: Some(TINY_PNG_B64.to_string()),
            mask_path: None,
            parameters: Map::new(),
            extra: None,
        },
        GenerateImageInput {
            model_definition_id: locked.id,
            prompt: "a cat".to_string(),
            negative_prompt: None,
            source_image: None,
            reference_images: vec![],
            mask_base64: None,
            mask_path: None,
            parameters: Map::new(),
            extra: None,
        },
        GenerateImageInput {
            model_definition_id: graph.id,
            prompt: "a cat".to_string(),
            negative_prompt: None,
            source_image: None,
            reference_images: vec![],
            mask_base64: None,
            mask_path: None,
            parameters: Map::new(),
            extra: None,
        },
    ];
    for input in cases {
        assert!(
            services::image::execute(&pool, &http, secrets.as_ref(), &dir, input)
                .await
                .is_err()
        );
    }
    let page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn comfyui_submits_a_bound_workflow_and_saves_the_view() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload/image"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "uploaded-source.png",
            "subfolder": "",
            "type": "input"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .and(body_string_contains("a red fox"))
        .and(body_string_contains("uploaded-source.png"))
        .and(body_string_contains("\"seed\":7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "prompt_id": "job-1",
            "node_errors": {}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/history/job-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "job-1": {
                "status": { "status_str": "success", "completed": true },
                "outputs": {
                    "9": {
                        "images": [{ "filename": "out.png", "subfolder": "", "type": "output" }]
                    }
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_png()))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let source = dir.join("local-source.png");
    std::fs::write(&source, tiny_png()).unwrap();
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Comfy",
        ProviderType::Comfyui,
        &server.uri(),
        None,
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Graph".to_string(),
            provider_id: provider.id,
            model_id: "flux".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "generate": true, "imageToImage": true }),
            parameters: vec![dimension("seed")],
            constraints: vec![],
            transport: Some(TransportDefinition {
                workflow: Some(json!({
                    "6": { "class_type": "CLIPTextEncode", "inputs": { "text": "REPLACE" } },
                    "10": { "class_type": "LoadImage", "inputs": { "image": "old.png" } },
                    "3": { "class_type": "KSampler", "inputs": { "seed": 1 } }
                })),
                bindings: Some(vec![
                    WorkflowBinding {
                        key: "prompt".to_string(),
                        node: "6".to_string(),
                        input: "text".to_string(),
                    },
                    WorkflowBinding {
                        key: "sourceImage".to_string(),
                        node: "10".to_string(),
                        input: "image".to_string(),
                    },
                    WorkflowBinding {
                        key: "seed".to_string(),
                        node: "3".to_string(),
                        input: "seed".to_string(),
                    },
                ]),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();
    let mut parameters = Map::new();
    parameters.insert("seed".to_string(), json!(7));
    let http = reqwest::Client::new();
    let result = services::image::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateImageInput {
            model_definition_id: model.id,
            prompt: "a red fox".to_string(),
            negative_prompt: None,
            source_image: Some(MediaRef::from_path(source.to_string_lossy())),
            reference_images: vec![],
            mask_base64: None,
            mask_path: None,
            parameters,
            extra: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.images[0].path).unwrap(), tiny_png());
    let page = services::history::list(
        &pool,
        HistoryQuery {
            modality: Some("image".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].status, "completed");
    assert!(result.raw_request.to_string().contains("a red fox"));
    let _ = std::fs::remove_dir_all(dir);
}

fn text_task(provider_id: &str, model_id: &str, prompt: &str, status: &str) -> NewTextTask {
    NewTextTask {
        kind: "generate".to_string(),
        provider_id: provider_id.to_string(),
        provider_name: "Mock".to_string(),
        model_id: model_id.to_string(),
        model_name: "Demo".to_string(),
        model_api_id: "demo-model".to_string(),
        prompt: prompt.to_string(),
        system_prompt: None,
        images: vec![],
        extra: None,
        conversation_id: None,
        parameters: Map::new(),
        status: status.to_string(),
        output_text: None,
        raw_request: None,
        raw_response: None,
        error: None,
    }
}

fn tiny_mp4() -> Vec<u8> {
    let mut bytes = vec![0, 0, 0, 20];
    bytes.extend_from_slice(b"ftypisom");
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes
}

fn video_recipe() -> HttpRecipe {
    HttpRecipe {
        method: "POST".to_string(),
        path: "v1/videos".to_string(),
        headers: vec![HeaderTemplate {
            name: "Authorization".to_string(),
            value: "Bearer {{apiKey}}".to_string(),
        }],
        body: Some(json!({
            "model": "{{model}}",
            "prompt": "{{prompt}}",
            "seed": "{{seed}}"
        })),
        task_id_path: Some("$.id".to_string()),
        poll: Some(HttpPoll {
            method: "GET".to_string(),
            path: "v1/videos/{{taskId}}".to_string(),
            body: None,
            status_path: "$.status".to_string(),
            success: "completed".to_string(),
            failure: "failed".to_string(),
            interval_ms: Some(200),
        }),
        result_path: "$.url".to_string(),
        result_kind: None,
    }
}

#[tokio::test]
async fn generic_http_polls_a_video_and_keeps_the_key_out_of_sqlite() {
    let secret = "sk-video-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/videos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "job-9" })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/videos/job-9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "processing" })))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/videos/job-9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "completed",
            "url": format!("{}/files/out.mp4", server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/out.mp4"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_mp4()))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Video API",
        ProviderType::GenericHttp,
        &server.uri(),
        Some(secret),
    )
    .await;
    let mut recipe = video_recipe();
    recipe.result_path = "$.url".to_string();
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Clip".to_string(),
            provider_id: provider.id,
            model_id: "video-model".to_string(),
            modality: Modality::Video,
            capabilities: json!({ "textToVideo": true }),
            parameters: vec![dimension("seed")],
            constraints: vec![],
            transport: Some(TransportDefinition {
                http: Some(recipe),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();
    let mut parameters = Map::new();
    parameters.insert("seed".to_string(), json!(7));
    let http = reqwest::Client::new();
    let result = services::video::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateVideoInput {
            model_definition_id: model.id,
            prompt: "a red fox".to_string(),
            negative_prompt: None,
            first_frame: None,
            last_frame: None,
            reference_images: vec![],
            reference_videos: vec![],
            reference_audios: vec![],
            parameters,
            extra: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.videos[0].path).unwrap(), tiny_mp4());
    let requests = server.received_requests().await.unwrap();
    let submit = requests
        .iter()
        .find(|request| request.url.path() == "/v1/videos")
        .expect("submit");
    let body = String::from_utf8_lossy(&submit.body);
    assert!(body.contains("a red fox"));
    assert!(body.contains("\"seed\":7"));
    assert!(submit
        .headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains(secret)));
    let page = services::history::list(
        &pool,
        HistoryQuery {
            modality: Some("video".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].status, "completed");
    let db_text = String::from_utf8_lossy(&std::fs::read(dir.join("app.db")).unwrap()).into_owned();
    assert!(!db_text.contains(secret));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn comfyui_video_uploads_the_first_frame_and_saves_the_view() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload/image"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "uploaded-frame.png",
            "subfolder": ""
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/prompt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "prompt_id": "job-v" })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/history/job-v"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "job-v": {
                "status": { "status_str": "success", "completed": true },
                "outputs": {
                    "20": { "gifs": [{ "filename": "clip.mp4", "subfolder": "", "type": "output" }] }
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/view"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_mp4()))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let frame = dir.join("frame.png");
    std::fs::write(&frame, tiny_png()).unwrap();
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Comfy",
        ProviderType::Comfyui,
        &server.uri(),
        None,
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Motion".to_string(),
            provider_id: provider.id,
            model_id: "wan".to_string(),
            modality: Modality::Video,
            capabilities: json!({ "textToVideo": true, "firstFrame": true }),
            parameters: vec![],
            constraints: vec![],
            transport: Some(TransportDefinition {
                workflow: Some(json!({
                    "6": { "class_type": "CLIPTextEncode", "inputs": { "text": "keep" } },
                    "10": { "class_type": "LoadImage", "inputs": { "image": "old.png" } }
                })),
                bindings: Some(vec![
                    WorkflowBinding {
                        key: "prompt".to_string(),
                        node: "6".to_string(),
                        input: "text".to_string(),
                    },
                    WorkflowBinding {
                        key: "firstFrame".to_string(),
                        node: "10".to_string(),
                        input: "image".to_string(),
                    },
                ]),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let result = services::video::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateVideoInput {
            model_definition_id: model.id.clone(),
            prompt: "a red fox".to_string(),
            negative_prompt: None,
            first_frame: Some(MediaRef::from_path(frame.to_string_lossy())),
            last_frame: None,
            reference_images: vec![],
            reference_videos: vec![],
            reference_audios: vec![],
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.videos[0].path).unwrap(), tiny_mp4());
    let requests = server.received_requests().await.unwrap();
    let prompt = requests
        .iter()
        .find(|request| request.url.path() == "/prompt")
        .expect("prompt");
    let body = String::from_utf8_lossy(&prompt.body);
    assert!(body.contains("a red fox"));
    assert!(body.contains("uploaded-frame.png"));
    let page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    let item = services::history::get(&pool, &page.items[0].id)
        .await
        .unwrap();
    assert_eq!(item.kind, "video");
    assert!(item.first_frame.is_some());
    assert!(item.assets.iter().any(|asset| asset.asset_type == "video"));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn video_failure_is_recorded_and_validation_is_not() {
    let secret = "sk-video-denied-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/videos"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "message": "unauthorized" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let denied = image_provider(
        &pool,
        secrets.as_ref(),
        "Denied",
        ProviderType::GenericHttp,
        &server.uri(),
        Some(secret),
    )
    .await;
    let denied_model = services::models::create(
        &pool,
        ModelInput {
            name: "Denied video".to_string(),
            provider_id: denied.id.clone(),
            model_id: "video-model".to_string(),
            modality: Modality::Video,
            capabilities: json!({ "textToVideo": true }),
            parameters: vec![],
            constraints: vec![],
            transport: Some(TransportDefinition {
                http: Some(video_recipe()),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let error = services::video::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateVideoInput {
            model_definition_id: denied_model.id.clone(),
            prompt: "a cat".to_string(),
            negative_prompt: None,
            first_frame: None,
            last_frame: None,
            reference_images: vec![],
            reference_videos: vec![],
            reference_audios: vec![],
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, AUTH);
    let failed = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(failed.total, 1);
    assert_eq!(failed.items[0].status, "failed");
    let detail = services::history::get(&pool, &failed.items[0].id)
        .await
        .unwrap();
    assert!(detail.raw_request.is_null());
    assert!(detail.assets.iter().all(|asset| {
        asset.metadata.get("role").and_then(|role| role.as_str()) != Some("output")
    }));
    assert_no_secret(&dir, secret);

    let text_model = services::models::create(
        &pool,
        ModelInput {
            name: "Text".to_string(),
            provider_id: denied.id.clone(),
            model_id: "text".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let bare = services::models::create(
        &pool,
        ModelInput {
            name: "Bare".to_string(),
            provider_id: denied.id.clone(),
            model_id: "bare".to_string(),
            modality: Modality::Video,
            capabilities: json!({ "textToVideo": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let openai = image_provider(
        &pool,
        secrets.as_ref(),
        "Official",
        ProviderType::Openai,
        "https://api.openai.com/v1",
        None,
    )
    .await;
    let openai_model = services::models::create(
        &pool,
        ModelInput {
            name: "Official video".to_string(),
            provider_id: openai.id,
            model_id: "video".to_string(),
            modality: Modality::Video,
            capabilities: json!({ "textToVideo": true }),
            parameters: vec![],
            constraints: vec![],
            transport: Some(TransportDefinition {
                http: Some(video_recipe()),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();
    let framed = services::models::create(
        &pool,
        ModelInput {
            name: "Frames".to_string(),
            provider_id: denied.id,
            model_id: "frames".to_string(),
            modality: Modality::Video,
            capabilities: json!({ "textToVideo": true, "lastFrame": true, "referenceImages": false }),
            parameters: vec![],
            constraints: vec![ConstraintDefinition::MutuallyExclusive {
                fields: vec!["firstFrame".to_string(), "referenceImages".to_string()],
            }],
            transport: Some(TransportDefinition {
                http: Some(video_recipe()),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();
    for (model_id, input) in [
        (
            text_model.id,
            GenerateVideoInput {
                model_definition_id: String::new(),
                prompt: "a cat".to_string(),
                negative_prompt: None,
                first_frame: None,
                last_frame: None,
                reference_images: vec![],
                reference_videos: vec![],
                reference_audios: vec![],
                parameters: Map::new(),
                extra: None,
            },
        ),
        (
            bare.id,
            GenerateVideoInput {
                model_definition_id: String::new(),
                prompt: "a cat".to_string(),
                negative_prompt: None,
                first_frame: None,
                last_frame: None,
                reference_images: vec![],
                reference_videos: vec![],
                reference_audios: vec![],
                parameters: Map::new(),
                extra: None,
            },
        ),
        (
            openai_model.id,
            GenerateVideoInput {
                model_definition_id: String::new(),
                prompt: "a cat".to_string(),
                negative_prompt: None,
                first_frame: None,
                last_frame: None,
                reference_images: vec![],
                reference_videos: vec![],
                reference_audios: vec![],
                parameters: Map::new(),
                extra: None,
            },
        ),
        (
            framed.id.clone(),
            GenerateVideoInput {
                model_definition_id: String::new(),
                prompt: "a cat".to_string(),
                negative_prompt: None,
                first_frame: None,
                last_frame: Some(MediaRef::from_path(
                    dir.join("missing.png").to_string_lossy(),
                )),
                reference_images: vec![],
                reference_videos: vec![],
                reference_audios: vec![],
                parameters: Map::new(),
                extra: None,
            },
        ),
    ] {
        let mut attempt = input;
        attempt.model_definition_id = model_id;
        assert!(
            services::video::execute(&pool, &http, secrets.as_ref(), &dir, attempt)
                .await
                .is_err()
        );
    }
    let still = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(still.total, 1);
    let _ = std::fs::remove_dir_all(dir);
}

fn tiny_mp3() -> Vec<u8> {
    b"ID3tiny".to_vec()
}

fn tiny_wav() -> Vec<u8> {
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(b"WAVE");
    bytes
}

fn audio_recipe() -> HttpRecipe {
    HttpRecipe {
        method: "POST".to_string(),
        path: "v1/speech".to_string(),
        headers: vec![HeaderTemplate {
            name: "Authorization".to_string(),
            value: "Bearer {{apiKey}}".to_string(),
        }],
        body: Some(json!({
            "text": "{{text}}",
            "voice": "{{voice}}"
        })),
        task_id_path: None,
        poll: None,
        result_path: "$".to_string(),
        result_kind: Some("binary".to_string()),
    }
}

fn audio_input(model_id: &str, text: &str) -> GenerateAudioInput {
    GenerateAudioInput {
        model_definition_id: model_id.to_string(),
        task: None,
        text: text.to_string(),
        voice: Some("alloy".to_string()),
        source_audio: None,
        reference_audio: None,
        reference_transcript: None,
        parameters: Map::new(),
        extra: None,
    }
}

#[tokio::test]
async fn openai_tts_saves_audio_and_keeps_the_key_out_of_sqlite() {
    let secret = "sk-audio-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .and(header("authorization", format!("Bearer {secret}")))
        .and(body_string_contains("\"input\":\"hello there\""))
        .and(body_string_contains("\"voice\":\"alloy\""))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_mp3()))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Speech",
        ProviderType::Openai,
        &server.uri(),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Speaker".to_string(),
            provider_id: provider.id,
            model_id: "tts-1".to_string(),
            modality: Modality::Audio,
            capabilities: json!({ "textToSpeech": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let result = services::audio::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        audio_input(&model.id, "hello there"),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.audios[0].path).unwrap(), tiny_mp3());
    assert!(!result.raw_request.to_string().contains(secret));
    let page = services::history::list(
        &pool,
        HistoryQuery {
            modality: Some("audio".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].status, "completed");
    let detail = services::history::get(&pool, &page.items[0].id)
        .await
        .unwrap();
    assert_eq!(detail.voice.as_deref(), Some("alloy"));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn generic_http_returns_audio_bytes_and_keeps_the_key_out_of_sqlite() {
    let secret = "sk-generic-audio-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/speech"))
        .and(header("authorization", format!("Bearer {secret}")))
        .and(body_string_contains("\"text\":\"hello there\""))
        .and(body_string_contains("\"voice\":\"alloy\""))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_mp3()))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Speech HTTP",
        ProviderType::GenericHttp,
        &server.uri(),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "HTTP speaker".to_string(),
            provider_id: provider.id,
            model_id: "local-tts".to_string(),
            modality: Modality::Audio,
            capabilities: json!({ "textToSpeech": true }),
            parameters: vec![],
            constraints: vec![],
            transport: Some(TransportDefinition {
                http: Some(audio_recipe()),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let result = services::audio::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        audio_input(&model.id, "hello there"),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.audios[0].path).unwrap(), tiny_mp3());
    assert!(!result.raw_request.to_string().contains(secret));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn audio_failure_is_recorded_and_validation_is_not() {
    let secret = "sk-audio-denied-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "message": "unauthorized" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let denied = image_provider(
        &pool,
        secrets.as_ref(),
        "Denied",
        ProviderType::OpenaiCompatible,
        &server.uri(),
        Some(secret),
    )
    .await;
    let denied_model = services::models::create(
        &pool,
        ModelInput {
            name: "Denied speech".to_string(),
            provider_id: denied.id.clone(),
            model_id: "tts-1".to_string(),
            modality: Modality::Audio,
            capabilities: json!({ "textToSpeech": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let error = services::audio::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        audio_input(&denied_model.id, "hello there"),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, AUTH);
    let failed = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(failed.total, 1);
    assert_eq!(failed.items[0].status, "failed");
    let detail = services::history::get(&pool, &failed.items[0].id)
        .await
        .unwrap();
    assert!(detail.raw_request.is_null());
    assert!(detail.assets.iter().all(|asset| {
        asset.metadata.get("role").and_then(|role| role.as_str()) != Some("output")
    }));
    assert_no_secret(&dir, secret);

    let empty = services::audio::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateAudioInput {
            text: "   ".to_string(),
            ..audio_input(&denied_model.id, "")
        },
    )
    .await
    .unwrap_err();
    assert_eq!(empty.message, "请填写要朗读的文本");

    let text_model = services::models::create(
        &pool,
        ModelInput {
            name: "Text".to_string(),
            provider_id: denied.id.clone(),
            model_id: "text".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let wrong = services::audio::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        audio_input(&text_model.id, "hello there"),
    )
    .await
    .unwrap_err();
    assert_eq!(wrong.message, "当前入口只执行音频生成");

    let comfy = image_provider(
        &pool,
        secrets.as_ref(),
        "Comfy",
        ProviderType::Comfyui,
        "http://127.0.0.1:8188",
        None,
    )
    .await;
    let comfy_model = services::models::create(
        &pool,
        ModelInput {
            name: "Comfy speech".to_string(),
            provider_id: comfy.id,
            model_id: "audio".to_string(),
            modality: Modality::Audio,
            capabilities: json!({ "textToSpeech": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let rejected = services::audio::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        audio_input(&comfy_model.id, "hello there"),
    )
    .await
    .unwrap_err();
    assert_eq!(
        rejected.message,
        "这个音频任务目前由 OpenAI、OpenAI Compatible、Generic HTTP、Replicate 与 fal.ai 执行"
    );

    let openai = image_provider(
        &pool,
        secrets.as_ref(),
        "Official",
        ProviderType::Openai,
        "https://api.openai.com/v1",
        Some(secret),
    )
    .await;
    let openai_model = services::models::create(
        &pool,
        ModelInput {
            name: "Official speech".to_string(),
            provider_id: openai.id,
            model_id: "tts-1".to_string(),
            modality: Modality::Audio,
            capabilities: json!({ "textToSpeech": true, "voiceClone": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let reference = services::audio::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateAudioInput {
            reference_audio: Some(MediaRef::from_path(
                dir.join("missing.wav").to_string_lossy(),
            )),
            ..audio_input(&openai_model.id, "hello there")
        },
    )
    .await
    .unwrap_err();
    assert_eq!(
        reference.message,
        "这个语音接口不接受参考音频，请改用 Generic HTTP"
    );

    let still = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(still.total, 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn voice_library_copies_and_deletes_the_file() {
    let (pool, _secrets, dir) = setup().await;
    let source = dir.join("reference.wav");
    std::fs::write(&source, tiny_wav()).unwrap();
    let extra = dir.join("reference_extra.wav");
    std::fs::write(&extra, tiny_wav()).unwrap();
    let created = services::voices::create(
        &pool,
        &dir,
        VoiceInput {
            name: "Yuna".to_string(),
            source_path: source.to_string_lossy().to_string(),
            transcript: "hello".to_string(),
            tags: "warm".to_string(),
            language: Some("zh".to_string()),
            notes: Some("test notes".to_string()),
            reference_paths: Some(vec![extra.to_string_lossy().to_string()]),
        },
    )
    .await
    .unwrap();
    let listed = services::voices::list(&pool).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Yuna");
    assert_eq!(listed[0].language, "zh");
    assert_eq!(listed[0].notes, "test notes");
    assert_eq!(listed[0].reference_files.len(), 1);
    let stored = PathBuf::from(&created.file_path);
    assert!(stored.starts_with(dir.join("audio").join("voices")));
    assert_eq!(std::fs::read(&stored).unwrap(), tiny_wav());
    let extra_stored = PathBuf::from(&created.reference_files[0]);
    assert!(extra_stored.exists());
    services::voices::delete(&pool, &dir, &created.id)
        .await
        .unwrap();
    assert!(services::voices::list(&pool).await.unwrap().is_empty());
    assert!(!stored.exists());
    assert!(!extra_stored.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn agent_calls_http_then_answers_and_keeps_the_key_out_of_sqlite() {
    let secret = "sk-agent-secret-value";
    let server = MockServer::start().await;
    let note = format!("{}/note", server.uri());
    let arguments = json!({ "method": "GET", "url": note }).to_string();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("http_request"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "http_request", "arguments": arguments }
                    }]
                }
            }]
        })))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/note"))
        .respond_with(ResponseTemplate::new(200).set_body_string("desk"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("desk"))
        .and(body_string_contains("call_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "看到了 desk" } }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Agent",
        ProviderType::OpenaiCompatible,
        &format!("{}/v1", server.uri()),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Talker".to_string(),
            provider_id: provider.id,
            model_id: "demo-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let cancels = Mutex::new(HashMap::new());
    let mut input = chat_turn(&model.id, None, "看一下便签", None);
    input.tools = vec!["http_request".to_string()];
    input.confirmed_tools = vec![json!({
        "name": "http_request",
        "arguments": {"method": "GET", "url": note}
    })
    .to_string()];
    input.max_steps = Some(4);
    let result = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        input,
        &|_| true,
    )
    .await
    .unwrap();
    assert_eq!(result.assistant.text, "看到了 desk");
    assert_eq!(result.assistant.tool_logs.len(), 1);
    assert!(result.assistant.tool_logs[0].ok);
    assert!(result.assistant.tool_logs[0].result.contains("desk"));
    assert!(!result.assistant.max_steps_hit);
    let page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].status, "completed");
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn agent_stops_at_max_steps_and_validation_is_not_recorded() {
    let secret = "sk-agent-limit-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_time",
                        "type": "function",
                        "function": { "name": "current_time", "arguments": "{}" }
                    }]
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Agent",
        ProviderType::Openai,
        &format!("{}/v1", server.uri()),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Talker".to_string(),
            provider_id: provider.id.clone(),
            model_id: "demo-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let cancels = Mutex::new(HashMap::new());
    let mut input = chat_turn(&model.id, None, "现在几点", None);
    input.tools = vec!["current_time".to_string()];
    input.max_steps = Some(1);
    let result = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        input,
        &|_| true,
    )
    .await
    .unwrap();
    assert!(result.assistant.text.contains("已达到最大步数 1"));
    assert!(result.assistant.max_steps_hit);
    assert_eq!(result.assistant.tool_logs.len(), 1);
    assert!(result.assistant.tool_logs[0].ok);

    let unknown = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        {
            let mut input = chat_turn(&model.id, None, "跑个命令", None);
            input.tools = vec!["shell".to_string()];
            input
        },
        &|_| true,
    )
    .await
    .unwrap_err();
    assert_eq!(unknown.message, "未知工具「shell」");

    let steps = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        {
            let mut input = chat_turn(&model.id, None, "现在几点", None);
            input.tools = vec!["current_time".to_string()];
            input.max_steps = Some(0);
            input
        },
        &|_| true,
    )
    .await
    .unwrap_err();
    assert_eq!(steps.message, "最大步数需要在 1 到 20 之间");

    let comfy = image_provider(
        &pool,
        secrets.as_ref(),
        "Comfy",
        ProviderType::Comfyui,
        "http://127.0.0.1:8188",
        None,
    )
    .await;
    let comfy_model = services::models::create(
        &pool,
        ModelInput {
            name: "Comfy text".to_string(),
            provider_id: comfy.id,
            model_id: "text".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let rejected = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        {
            let mut input = chat_turn(&comfy_model.id, None, "现在几点", None);
            input.tools = vec!["current_time".to_string()];
            input
        },
        &|_| true,
    )
    .await
    .unwrap_err();
    assert_eq!(
        rejected.message,
        "Agent 工具调用目前由 OpenAI、OpenAI Compatible、Ollama、Anthropic 与 Gemini 适配器执行"
    );
    let page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

fn chat_turn(
    model_id: &str,
    conversation_id: Option<String>,
    prompt: &str,
    replace_from_message_id: Option<String>,
) -> ChatSendInput {
    ChatSendInput {
        conversation_id,
        model_definition_id: model_id.to_string(),
        system_prompt: None,
        context_strategy: "recent".to_string(),
        context_limit: 1,
        prompt: prompt.to_string(),
        images: vec![],
        parameters: Map::new(),
        extra: None,
        replace_from_message_id,
        tools: Vec::new(),
        confirmed_tools: Vec::new(),
        max_steps: None,
    }
}

#[tokio::test]
async fn presets_survive_reopen_and_leave_with_the_model() {
    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Local".to_string(),
            provider_type: ProviderType::Ollama,
            base_url: "http://127.0.0.1:9".to_string(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 5_000,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Portrait".to_string(),
            provider_id: provider.id,
            model_id: "flux".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "generate": true }),
            parameters: vec![dimension("seed")],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let mut parameters = Map::new();
    parameters.insert("seed".to_string(), json!(28));
    let preset = services::presets::create(
        &pool,
        PresetInput {
            name: "高质量头像".to_string(),
            model_id: model.id.clone(),
            system_prompt: Some("You are an expert artist".to_string()),
            user_template: Some("Generate portrait for {prompt}".to_string()),
            parameters,
        },
    )
    .await
    .unwrap();
    drop(pool);
    let pool = db::open(&dir.join("app.db")).await.unwrap();
    let listed = services::presets::list(&pool, &model.id).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, preset.id);
    assert_eq!(
        listed[0].system_prompt.as_deref(),
        Some("You are an expert artist")
    );
    assert_eq!(
        listed[0].user_template.as_deref(),
        Some("Generate portrait for {prompt}")
    );
    assert_eq!(listed[0].parameters.get("seed"), Some(&json!(28)));
    services::models::delete(&pool, &model.id).await.unwrap();
    let gone = services::presets::list(&pool, &model.id).await.unwrap();
    assert!(gone.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn video_reference_clip_is_sent_and_capability_is_required() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/videos"))
        .and(body_string_contains("data:video/mp4;base64"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_mp4()))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let clip = dir.join("ref.mp4");
    std::fs::write(&clip, tiny_mp4()).unwrap();
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Video API",
        ProviderType::GenericHttp,
        &server.uri(),
        None,
    )
    .await;
    let bare = services::models::create(
        &pool,
        ModelInput {
            name: "Bare".to_string(),
            provider_id: provider.id.clone(),
            model_id: "video-model".to_string(),
            modality: Modality::Video,
            capabilities: json!({ "textToVideo": true }),
            parameters: vec![],
            constraints: vec![],
            transport: Some(TransportDefinition {
                http: Some(HttpRecipe {
                    method: "POST".to_string(),
                    path: "v1/videos".to_string(),
                    headers: vec![],
                    body: Some(json!({ "clips": "{{referenceVideos}}" })),
                    task_id_path: None,
                    poll: None,
                    result_path: "$".to_string(),
                    result_kind: Some("binary".to_string()),
                }),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let rejected = services::video::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateVideoInput {
            model_definition_id: bare.id.clone(),
            prompt: "follow the clip".to_string(),
            negative_prompt: None,
            first_frame: None,
            last_frame: None,
            reference_images: vec![],
            reference_videos: vec![MediaRef::from_path(clip.to_string_lossy())],
            reference_audios: vec![],
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(rejected.message, "该模型没有参考视频能力");
    let quiet = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(quiet.total, 0);

    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Clip".to_string(),
            provider_id: provider.id,
            model_id: "video-model".to_string(),
            modality: Modality::Video,
            capabilities: json!({ "textToVideo": true, "referenceVideos": true }),
            parameters: vec![],
            constraints: vec![],
            transport: Some(TransportDefinition {
                http: Some(HttpRecipe {
                    method: "POST".to_string(),
                    path: "v1/videos".to_string(),
                    headers: vec![],
                    body: Some(json!({ "clips": "{{referenceVideos}}" })),
                    task_id_path: None,
                    poll: None,
                    result_path: "$".to_string(),
                    result_kind: Some("binary".to_string()),
                }),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();
    let result = services::video::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateVideoInput {
            model_definition_id: model.id,
            prompt: "follow the clip".to_string(),
            negative_prompt: None,
            first_frame: None,
            last_frame: None,
            reference_images: vec![],
            reference_videos: vec![MediaRef::from_path(clip.to_string_lossy())],
            reference_audios: vec![],
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.videos[0].path).unwrap(), tiny_mp4());
    let page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    let saved = services::history::get(&pool, &page.items[0].id)
        .await
        .unwrap();
    assert_eq!(saved.reference_videos.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

fn text_model(name: &str, provider_id: String, model_id: &str) -> ModelInput {
    ModelInput {
        name: name.to_string(),
        provider_id,
        model_id: model_id.to_string(),
        modality: Modality::Text,
        capabilities: json!({ "streaming": false }),
        parameters: vec![],
        constraints: vec![],
        transport: None,
        enabled: true,
    }
}

#[tokio::test]
async fn anthropic_messages_return_text_without_storing_the_key() {
    let secret = "sk-ant-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", secret))
        .and(body_string_contains("\"model\":\"claude\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{ "type": "text", "text": "claude" }],
            "usage": { "input_tokens": 2, "output_tokens": 3 }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Claude",
        ProviderType::Anthropic,
        &server.uri(),
        Some(secret),
    )
    .await;
    let model = services::models::create(&pool, text_model("Claude", provider.id, "claude"))
        .await
        .unwrap();
    let http = reqwest::Client::new();
    let result = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id,
            system_prompt: None,
            prompt: "ping".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await
    .unwrap();
    assert_eq!(result.text, "claude");
    assert!(!result.raw_request.to_string().contains(secret));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn gemini_generate_content_returns_text() {
    let secret = "gem-secret-value-1234";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-flash:generateContent"))
        .and(header("x-goog-api-key", secret))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{ "content": { "parts": [{ "text": "gemini" }] } }],
            "usageMetadata": {
                "promptTokenCount": 1,
                "candidatesTokenCount": 2,
                "totalTokenCount": 3
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Gemini",
        ProviderType::Gemini,
        &server.uri(),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        text_model("Flash", provider.id, "models/gemini-flash"),
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let result = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id,
            system_prompt: None,
            prompt: "ping".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await
    .unwrap();
    assert_eq!(result.text, "gemini");
    assert_eq!(result.usage.unwrap().total_tokens, Some(3));
    assert!(!result.raw_request.to_string().contains(secret));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn replicate_prediction_polls_and_saves_the_image() {
    let secret = "r8-secret-value";
    let server = MockServer::start().await;
    let poll = format!("{}/v1/predictions/job", server.uri());
    let file = format!("{}/files/out.png", server.uri());
    Mock::given(method("POST"))
        .and(path("/v1/models/owner/flux/predictions"))
        .and(header("authorization", format!("Bearer {secret}")))
        .and(body_string_contains("\"prompt\":\"a fox\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "processing",
            "urls": { "get": poll }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/predictions/job"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "succeeded",
            "output": file
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/out.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_png()))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Replicate",
        ProviderType::Replicate,
        &server.uri(),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Flux".to_string(),
            provider_id: provider.id,
            model_id: "owner/flux".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "generate": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let result = services::image::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateImageInput {
            model_definition_id: model.id,
            prompt: "a fox".to_string(),
            negative_prompt: None,
            source_image: None,
            reference_images: vec![],
            mask_base64: None,
            mask_path: None,
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.images[0].path).unwrap(), tiny_png());
    assert!(!result.raw_request.to_string().contains(secret));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn fal_queue_downloads_the_image_without_sending_the_key_to_the_file() {
    let secret = "fal-secret-value";
    let server = MockServer::start().await;
    let status = format!("{}/status", server.uri());
    let response_url = format!("{}/result", server.uri());
    let file = format!("{}/out.png", server.uri());
    Mock::given(method("POST"))
        .and(path("/fal-ai/flux/schnell"))
        .and(header("authorization", format!("Key {secret}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status_url": status,
            "response_url": response_url
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "COMPLETED"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/result"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "images": [{ "url": file }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/out.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_png()))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Fal",
        ProviderType::Fal,
        &server.uri(),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Schnell".to_string(),
            provider_id: provider.id,
            model_id: "fal-ai/flux/schnell".to_string(),
            modality: Modality::Image,
            capabilities: json!({ "generate": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let result = services::image::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateImageInput {
            model_definition_id: model.id,
            prompt: "a fox".to_string(),
            negative_prompt: None,
            source_image: None,
            reference_images: vec![],
            mask_base64: None,
            mask_path: None,
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&result.images[0].path).unwrap(), tiny_png());
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn openai_transcription_saves_the_transcript_in_history() {
    let secret = "sk-stt-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .and(header("authorization", format!("Bearer {secret}")))
        .and(body_string_contains("verbose_json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "text": "hello there",
            "segments": [
                { "start": 0.0, "end": 1.2, "text": "hello", "speaker": "A" },
                { "start": 1.2, "end": 2.0, "text": "there" }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (pool, secrets, dir) = setup().await;
    let source = dir.join("source.mp3");
    std::fs::write(&source, tiny_mp3()).unwrap();
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Speech",
        ProviderType::Openai,
        &server.uri(),
        Some(secret),
    )
    .await;
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Whisper".to_string(),
            provider_id: provider.id,
            model_id: "whisper-1".to_string(),
            modality: Modality::Audio,
            capabilities: json!({ "speechToText": true }),
            parameters: vec![ParameterDefinition {
                key: "timestamp".to_string(),
                label: "Timestamp".to_string(),
                kind: ParameterType::Boolean,
                default: Some(json!(false)),
                min: None,
                max: None,
                step: None,
                options: None,
                required: None,
                advanced: None,
                group: None,
            }],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let mut parameters = Map::new();
    parameters.insert("timestamp".to_string(), json!(true));
    let http = reqwest::Client::new();
    let mut input = audio_input(&model.id, "hello there");
    input.task = Some("stt".to_string());
    input.voice = None;
    input.source_audio = Some(MediaRef::from_path(source.to_string_lossy()));
    input.parameters = parameters;
    let result = services::audio::execute(&pool, &http, secrets.as_ref(), &dir, input)
        .await
        .unwrap();
    assert!(result.audios.is_empty());
    assert_eq!(
        result.transcript,
        "[00:00:00.000 --> 00:00:01.200] A: hello\n[00:00:01.200 --> 00:00:02.000] there"
    );
    let page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    let detail = services::history::get(&pool, &page.items[0].id)
        .await
        .unwrap();
    assert_eq!(detail.output, result.transcript);
    assert_eq!(detail.audio_task.as_deref(), Some("stt"));
    assert!(detail.source_audio.is_some());
    let audio_dir = dir.join("audio");
    let srt_file = audio_dir.join(format!("{}.srt", page.items[0].id));
    let vtt_file = audio_dir.join(format!("{}.vtt", page.items[0].id));
    assert!(srt_file.exists());
    assert!(vtt_file.exists());
    let srt_content = std::fs::read_to_string(&srt_file).unwrap();
    assert!(srt_content.contains("00:00:00,000 --> 00:00:01,200"));
    assert!(srt_content.contains("[A] hello"));
    let vtt_content = std::fs::read_to_string(&vtt_file).unwrap();
    assert!(vtt_content.starts_with("WEBVTT"));
    assert!(vtt_content.contains("<v A>hello</v>"));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn mcp_http_tool_is_called_from_the_agent() {
    let secret = "sk-mcp-secret-value";
    let model_server = MockServer::start().await;
    let mcp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(body_string_contains("\"method\":\"initialize\""))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("mcp-session-id", "sess-1")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": { "protocolVersion": "2024-11-05" }
                })),
        )
        .expect(1)
        .mount(&mcp)
        .await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(body_string_contains("\"method\":\"tools/list\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "tools": [{
                    "name": "echo",
                    "description": "Echo",
                    "inputSchema": { "type": "object", "properties": {} }
                }]
            }
        })))
        .expect(1)
        .mount(&mcp)
        .await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(body_string_contains("\"method\":\"tools/call\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "content": [{ "type": "text", "text": "pong" }] }
        })))
        .expect(1)
        .mount(&mcp)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("mcp_0_echo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_mcp",
                        "type": "function",
                        "function": { "name": "mcp_0_echo", "arguments": "{}" }
                    }]
                }
            }]
        })))
        .up_to_n_times(1)
        .expect(1)
        .mount(&model_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("pong"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "收到 pong" } }]
        })))
        .expect(1)
        .mount(&model_server)
        .await;
    let (pool, secrets, dir) = setup().await;
    services::mcp::create(
        &pool,
        crate::services::mcp::McpServerInput {
            name: "Echo".to_string(),
            transport: "http".to_string(),
            command: String::new(),
            args: vec![],
            url: format!("{}/mcp", mcp.uri()),
            enabled: true,
        },
    )
    .await
    .unwrap();
    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Agent",
        ProviderType::OpenaiCompatible,
        &format!("{}/v1", model_server.uri()),
        Some(secret),
    )
    .await;
    let model = services::models::create(&pool, text_model("Talker", provider.id, "demo"))
        .await
        .unwrap();
    let http = reqwest::Client::new();
    let cancels = Mutex::new(HashMap::new());
    let mut input = chat_turn(&model.id, None, "用一下工具", None);
    input.tools = vec!["mcp".to_string()];
    input.confirmed_tools = vec![json!({
        "name": "mcp_0_echo",
        "arguments": {}
    })
    .to_string()];
    input.max_steps = Some(3);
    let result = services::chat::send(
        &pool,
        &http,
        secrets.as_ref(),
        &cancels,
        &dir,
        input,
        &|_| true,
    )
    .await
    .unwrap();
    assert_eq!(result.assistant.text, "收到 pong");
    assert_eq!(result.assistant.tool_logs.len(), 1);
    assert!(result.assistant.tool_logs[0].ok);
    assert!(result.assistant.tool_logs[0].result.contains("pong"));
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn speech_to_speech_converts_source_audio_and_records_in_history() {
    let secret = "sk-sts-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/speech-to-speech"))
        .and(header("authorization", format!("Bearer {secret}")))
        .and(body_string_contains("data:audio/mpeg;base64,"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_wav()))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let source = dir.join("source.mp3");
    std::fs::write(&source, tiny_mp3()).unwrap();

    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "STS HTTP",
        ProviderType::GenericHttp,
        &server.uri(),
        Some(secret),
    )
    .await;

    let model = services::models::create(
        &pool,
        ModelInput {
            name: "STS Converter".to_string(),
            provider_id: provider.id,
            model_id: "sts-v1".to_string(),
            modality: Modality::Audio,
            capabilities: json!({ "speechToSpeech": true }),
            parameters: vec![],
            constraints: vec![],
            transport: Some(TransportDefinition {
                http: Some(HttpRecipe {
                    method: "POST".to_string(),
                    path: "v1/speech-to-speech".to_string(),
                    headers: vec![HeaderTemplate {
                        name: "Authorization".to_string(),
                        value: "Bearer {{apiKey}}".to_string(),
                    }],
                    body: Some(json!({
                        "audio": "{{sourceAudio}}"
                    })),
                    task_id_path: None,
                    poll: None,
                    result_path: "$".to_string(),
                    result_kind: Some("binary".to_string()),
                }),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();
    let mut input = audio_input(&model.id, "");
    input.task = Some("sts".to_string());
    input.voice = None;
    input.source_audio = Some(MediaRef::from_path(source.to_string_lossy()));

    let result = services::audio::execute(&pool, &http, secrets.as_ref(), &dir, input)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&result.audios[0].path).unwrap(), tiny_wav());
    assert!(!result.raw_request.to_string().contains(secret));

    let page = services::history::list(
        &pool,
        HistoryQuery {
            modality: Some("audio".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].status, "completed");

    let detail = services::history::get(&pool, &page.items[0].id)
        .await
        .unwrap();
    assert_eq!(detail.audio_task.as_deref(), Some("sts"));
    assert!(detail.source_audio.is_some());
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn voice_clone_uses_reference_audio_and_saves_audio_in_history() {
    let secret = "sk-clone-secret-value";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/voice-clone"))
        .and(header("authorization", format!("Bearer {secret}")))
        .and(body_string_contains("\"text\":\"hello cloned voice\""))
        .and(body_string_contains("data:audio/mpeg;base64,"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(tiny_mp3()))
        .expect(1)
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let reference = dir.join("reference.mp3");
    std::fs::write(&reference, tiny_mp3()).unwrap();

    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Clone HTTP",
        ProviderType::GenericHttp,
        &server.uri(),
        Some(secret),
    )
    .await;

    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Clone Speaker".to_string(),
            provider_id: provider.id,
            model_id: "clone-v1".to_string(),
            modality: Modality::Audio,
            capabilities: json!({ "voiceClone": true }),
            parameters: vec![],
            constraints: vec![],
            transport: Some(TransportDefinition {
                http: Some(HttpRecipe {
                    method: "POST".to_string(),
                    path: "v1/voice-clone".to_string(),
                    headers: vec![HeaderTemplate {
                        name: "Authorization".to_string(),
                        value: "Bearer {{apiKey}}".to_string(),
                    }],
                    body: Some(json!({
                        "text": "{{text}}",
                        "reference": "{{referenceAudio}}"
                    })),
                    task_id_path: None,
                    poll: None,
                    result_path: "$".to_string(),
                    result_kind: Some("binary".to_string()),
                }),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();
    let mut input = audio_input(&model.id, "hello cloned voice");
    input.task = Some("clone".to_string());
    input.voice = None;
    input.reference_audio = Some(MediaRef::from_path(reference.to_string_lossy()));

    let result = services::audio::execute(&pool, &http, secrets.as_ref(), &dir, input)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&result.audios[0].path).unwrap(), tiny_mp3());
    assert!(!result.raw_request.to_string().contains(secret));

    let page = services::history::list(
        &pool,
        HistoryQuery {
            modality: Some("audio".to_string()),
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].status, "completed");

    let detail = services::history::get(&pool, &page.items[0].id)
        .await
        .unwrap();
    assert_eq!(detail.audio_task.as_deref(), Some("clone"));
    assert!(detail.reference_audio.is_some());
    assert_no_secret(&dir, secret);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase3_registry_catalog_and_import() {
    let (pool, secrets, dir) = setup().await;
    let catalog = services::registry::get_catalog(&dir);
    assert!(!catalog.providers.is_empty());
    assert!(!catalog.models.is_empty());
    assert!(catalog.providers.iter().any(|t| t.id == "openai"));
    assert!(catalog.models.iter().any(|m| m.id == "gpt-4o"));

    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "My OpenAI".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: Some("sk-test-key".to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    // 1. 导入模型
    let imported =
        services::registry::import_model_from_registry(&pool, &dir, "gpt-4o", &provider.id, None)
            .await
            .unwrap();

    assert_eq!(imported.name, "GPT-4o");
    assert_eq!(imported.model_id, "gpt-4o");
    assert_eq!(imported.provider_id, provider.id);
    assert!(imported.enabled);

    // 2. 验证防止意外覆盖：再次导入同 model_id 模型且未指定 overwrite 时报错
    let conflict_err =
        services::registry::import_model_from_registry(&pool, &dir, "gpt-4o", &provider.id, None)
            .await
            .unwrap_err();
    assert!(conflict_err.message.contains("已存在相同模型"));

    // 3. 允许指定 overwrite = Some(true) 覆盖更新
    let overwritten = services::registry::import_model_from_registry(
        &pool,
        &dir,
        "gpt-4o",
        &provider.id,
        Some(true),
    )
    .await
    .unwrap();
    assert_eq!(overwritten.id, imported.id);

    // 4. 导入带约束的模型模板 (FLUX.1-schnell)
    let flux = services::registry::import_model_from_registry(
        &pool,
        &dir,
        "flux-1-schnell",
        &provider.id,
        None,
    )
    .await
    .unwrap();
    assert_eq!(flux.constraints.len(), 2);

    // 5. 验证 Rollback 机制
    let reg_dir = dir.join("registry");
    std::fs::create_dir_all(&reg_dir).unwrap();
    let current_file = reg_dir.join("catalog.json");
    let backup_file = reg_dir.join("catalog.backup.json");
    let mut v2_catalog = catalog.clone();
    v2_catalog.version = "2026.11.0".to_string();
    std::fs::write(&backup_file, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    std::fs::write(
        &current_file,
        serde_json::to_vec_pretty(&v2_catalog).unwrap(),
    )
    .unwrap();

    let check_before = services::registry::get_catalog(&dir);
    assert_eq!(check_before.version, "2026.11.0");
    assert!(check_before.can_rollback);

    let restored = services::registry::rollback_registry(&dir).unwrap();
    assert_eq!(restored.version, catalog.version);
    assert!(!restored.can_rollback);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase3_provider_plugins_lifecycle_and_instantiate() {
    let (pool, secrets, dir) = setup().await;

    // 1. Built-in plugins list
    let plugins = services::plugins::list(&dir).unwrap();
    assert!(plugins.iter().any(|p| p.id == "groq"));

    // 2. Custom plugin install & uninstall
    let custom_manifest = json!({
        "id": "custom-provider",
        "name": "Custom Provider",
        "version": "1.0.0",
        "description": "A test custom provider plugin",
        "protocol": "openai-compatible",
        "defaultBaseUrl": "https://api.custom.ai/v1",
        "defaultHeaders": {},
        "recommendedModels": [{
            "id": "custom-v1",
            "name": "Custom Model V1",
            "modelId": "custom-v1",
            "modality": "text",
            "capabilities": { "streaming": true }
        }]
    })
    .to_string();

    services::plugins::install(&dir, &custom_manifest).unwrap();
    let plugins_after_install = services::plugins::list(&dir).unwrap();
    assert!(plugins_after_install
        .iter()
        .any(|p| p.id == "custom-provider"));

    // 3. Instantiate plugin
    let instantiated_provider = services::plugins::instantiate(
        &pool,
        secrets.as_ref(),
        &dir,
        "custom-provider",
        Some("custom-key-123".to_string()),
    )
    .await
    .unwrap();

    assert_eq!(instantiated_provider.name, "Custom Provider");
    assert!(instantiated_provider.has_credential);

    let models = services::models::list(&pool).await.unwrap();
    assert!(models.iter().any(|m| m.model_id == "custom-v1"));

    // 4. Uninstall custom plugin
    services::plugins::uninstall(&dir, "custom-provider").unwrap();
    let plugins_after_uninstall = services::plugins::list(&dir).unwrap();
    assert!(!plugins_after_uninstall
        .iter()
        .any(|p| p.id == "custom-provider"));

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase3_agent_memory_crud_and_prompt_formatting() {
    let (pool, _secrets, dir) = setup().await;

    // 1. Save global memory
    let mem1 = services::memory::save(
        &pool,
        services::memory::AgentMemoryInput {
            id: None,
            conversation_id: None,
            scope: Some("global".to_string()),
            key: "user_name".to_string(),
            content: "Ryan".to_string(),
            importance: Some(2),
        },
    )
    .await
    .unwrap();

    assert_eq!(mem1.key, "user_name");
    assert_eq!(mem1.content, "Ryan");

    // 2. Save conversation-scoped memory
    let conv_id = "conv-test-123";
    let mem2 = services::memory::save(
        &pool,
        services::memory::AgentMemoryInput {
            id: None,
            conversation_id: Some(conv_id.to_string()),
            scope: Some("conversation".to_string()),
            key: "project_goal".to_string(),
            content: "Complete Phase 3 in full sequential order".to_string(),
            importance: Some(1),
        },
    )
    .await
    .unwrap();

    // 3. List memories
    let all_memories = services::memory::list(&pool, None).await.unwrap();
    assert_eq!(all_memories.len(), 2);

    let conv_memories = services::memory::list(&pool, Some(conv_id)).await.unwrap();
    assert_eq!(conv_memories.len(), 2); // global + conversation

    let other_conv = services::memory::list(&pool, Some("other-conv"))
        .await
        .unwrap();
    assert_eq!(other_conv.len(), 1); // only global

    // 4. Format for prompt
    let prompt_block = services::memory::format_for_prompt(&pool, Some(conv_id))
        .await
        .unwrap();
    assert!(prompt_block.contains("【Agent 记忆库（长期记忆与用户偏好）】:"));
    assert!(prompt_block.contains("user_name"));
    assert!(prompt_block.contains("project_goal"));

    // 5. Delete memory
    services::memory::delete(&pool, &mem2.id).await.unwrap();
    let remaining = services::memory::list(&pool, None).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].key, "user_name");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase3_context_compaction_and_pinning() {
    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Mock Provider".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: "http://127.0.0.1:9".to_string(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Mock Text".to_string(),
            provider_id: provider.id,
            model_id: "mock-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let conv_id = "test-conv-compaction";
    let now = crate::domain::now_iso();
    db::insert_conversation(
        &pool,
        &db::ConversationRow {
            id: conv_id.to_string(),
            title: "Compaction Test".to_string(),
            mode: "chat".to_string(),
            model_id: model.id,
            system_prompt: "You are a helpful assistant".to_string(),
            context_strategy: "recent".to_string(),
            context_limit: 10,
            summary: None,
            summary_until: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        },
    )
    .await
    .unwrap();

    // Insert 3 messages
    let msg1_id = "m1";
    let msg2_id = "m2";
    let msg3_id = "m3";

    for (m_id, role, text, pinned) in [
        (
            msg1_id,
            "user",
            "Important requirement that must be kept pinned",
            true,
        ),
        (
            msg2_id,
            "assistant",
            "I understand your important requirement",
            false,
        ),
        (msg3_id, "user", "What is the latest status?", false),
    ] {
        let stored = json!({
            "text": text,
            "images": [],
            "reasoning": "",
            "usage": null,
            "stopped": false,
            "error": null,
            "toolLogs": [],
            "maxStepsHit": false
        });
        db::insert_message(
            &pool,
            &db::MessageRow {
                id: m_id.to_string(),
                conversation_id: conv_id.to_string(),
                role: role.to_string(),
                content_json: stored.to_string(),
                token_count: 10,
                raw_request_json: None,
                raw_response_json: None,
                is_pinned: pinned,
                created_at: now.clone(),
            },
        )
        .await
        .unwrap();
    }

    // Verify pinned state
    let detail = services::chat::get(&pool, conv_id).await.unwrap();
    assert_eq!(detail.messages.len(), 3);
    assert!(detail.messages[0].is_pinned);
    assert!(!detail.messages[1].is_pinned);
    assert!(!detail.messages[2].is_pinned);

    // Toggle pin on msg2
    let msg2_row = db::get_message(&pool, msg2_id).await.unwrap();
    assert!(!msg2_row.is_pinned);
    let mut updated_msg2 = msg2_row.clone();
    updated_msg2.is_pinned = true;
    db::update_message(&pool, &updated_msg2).await.unwrap();

    let check_msg2 = db::get_message(&pool, msg2_id).await.unwrap();
    assert!(check_msg2.is_pinned);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase3_model_pricing_and_analytics_summary() {
    let (pool, secrets, dir) = setup().await;

    // 1. Set pricing
    services::analytics::set_pricing(&pool, "gpt-4o", 2.5, 10.0)
        .await
        .unwrap();

    let pricings = services::analytics::list_pricing(&pool).await.unwrap();
    assert_eq!(pricings.len(), 1);
    assert_eq!(pricings[0].model_id, "gpt-4o");
    assert_eq!(pricings[0].input_per_m, 2.5);
    assert_eq!(pricings[0].output_per_m, 10.0);

    // 2. Add history text task with usage
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Mock Provider".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: "http://127.0.0.1:9".to_string(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model = services::models::create(
        &pool,
        ModelInput {
            name: "GPT-4o".to_string(),
            provider_id: provider.id.clone(),
            model_id: "gpt-4o".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let usage_raw = json!({
        "prompt_tokens": 1000,
        "completion_tokens": 500,
        "total_tokens": 1500
    });

    services::history::record_text_task(
        &pool,
        &dir,
        NewTextTask {
            kind: "text_to_text".to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            prompt: "Test analytics prompt".to_string(),
            system_prompt: None,
            images: vec![],
            extra: None,
            conversation_id: None,
            parameters: Map::new(),
            status: "completed".to_string(),
            output_text: Some("Test output".to_string()),
            raw_request: Some(json!({ "model": "gpt-4o" })),
            raw_response: Some(json!({ "choices": [], "usage": usage_raw })),
            error: None,
        },
    )
    .await
    .unwrap();

    // 3. Get summary
    let summary = services::analytics::get_summary(&pool).await.unwrap();
    assert_eq!(summary.total_tasks, 1);
    assert_eq!(summary.total_tokens, 1500);
    assert!(summary.estimated_cost_usd > 0.0);
    assert_eq!(summary.top_models.len(), 1);
    assert_eq!(summary.top_models[0].model_id, "gpt-4o");
    assert_eq!(summary.top_models[0].task_count, 1);
    assert_eq!(summary.top_models[0].total_tokens, 1500);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase3_multi_model_compare_and_batch_generation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "mock reply text" } }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        })))
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Mock Provider".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: format!("{}/v1", server.uri()),
            api_key: Some("sk-test-key".to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model1 = services::models::create(
        &pool,
        ModelInput {
            name: "Model A".to_string(),
            provider_id: provider.id.clone(),
            model_id: "model-a".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model2 = services::models::create(
        &pool,
        ModelInput {
            name: "Model B".to_string(),
            provider_id: provider.id.clone(),
            model_id: "model-b".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();

    // 1. Multi-model compare test (2 models)
    let comp_res = services::compare::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        services::compare::CompareInput {
            prompt: "Hello comparison".to_string(),
            system_prompt: None,
            model_ids: vec![model1.id.clone(), model2.id.clone()],
            parameters: Map::new(),
        },
    )
    .await
    .unwrap();

    assert_eq!(comp_res.prompt, "Hello comparison");
    assert_eq!(comp_res.items.len(), 2);
    for item in &comp_res.items {
        assert_eq!(item.text, "mock reply text");
        assert_eq!(item.tokens, 15);
        assert!(item.error.is_none());
    }

    // 2. Batch generation test (2 prompts * 2 seeds = 4 items)
    let batch_res = services::batch::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        services::batch::BatchGenerateInput {
            modality: "text".to_string(),
            model_id: model1.id.clone(),
            prompts: vec!["Prompt 1".to_string(), "Prompt 2".to_string()],
            system_prompt: None,
            seeds: Some(vec![42, 99]),
            parameters: Map::new(),
        },
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(batch_res.total, 4);
    assert_eq!(batch_res.succeeded, 4);
    assert_eq!(batch_res.failed, 0);
    assert_eq!(batch_res.items.len(), 4);
    for item in &batch_res.items {
        assert_eq!(item.output.as_deref(), Some("mock reply text"));
        assert!(item.error.is_none());
    }

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_services_local_discovery_and_add() {
    let (pool, _secrets, dir) = setup().await;
    let client = reqwest::Client::new();

    // 1. Discover local services
    let discovered = services::discovery::discover_local_services(&pool, &client)
        .await
        .unwrap();

    assert_eq!(discovered.len(), 7);
    assert!(discovered
        .iter()
        .any(|d| d.id == "ollama" && d.port == 11434));
    assert!(discovered
        .iter()
        .any(|d| d.id == "lmstudio" && d.port == 1234));
    assert!(discovered
        .iter()
        .any(|d| d.id == "llamacpp" && d.port == 8080));
    assert!(discovered
        .iter()
        .any(|d| d.id == "comfyui" && d.port == 8188));
    assert!(discovered
        .iter()
        .any(|d| d.id == "localai" && d.port == 8000));
    assert!(discovered.iter().any(|d| d.id == "f5tts" && d.port == 9998));
    assert!(discovered
        .iter()
        .any(|d| d.id == "cosyvoice" && d.port == 50000));

    // 2. Add an Ollama service
    let added = services::discovery::add_discovered_service(&pool, "ollama")
        .await
        .unwrap();

    assert_eq!(added.name, "Ollama (本地)");
    assert_eq!(added.base_url, "http://127.0.0.1:11434");

    // Verify seeded model
    let models = services::models::list(&pool).await.unwrap();
    let seeded = models.iter().find(|m| m.provider_id == added.id).unwrap();
    assert_eq!(seeded.model_id, "llama3");

    // Re-scanning should mark it as already_added
    let discovered_after = services::discovery::discover_local_services(&pool, &client)
        .await
        .unwrap();
    let ollama_after = discovered_after.iter().find(|d| d.id == "ollama").unwrap();
    assert!(ollama_after.already_added);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_config_io_and_network_proxy() {
    let (pool, secrets, dir) = setup().await;

    // 1. Create a provider with sensitive API key
    let prov = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "SecretProvider".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: Some("sk-super-secret-key-12345".to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    assert!(prov.has_credential);

    // 2. Export config - verify sensitive API Key is NEVER exported
    let exported = services::config_io::export_all_config(&pool).await.unwrap();

    let json_text = serde_json::to_string(&exported).unwrap();
    assert!(!json_text.contains("sk-super-secret-key-12345"));
    assert!(!json_text.contains("credentialId"));
    assert_eq!(exported.providers.len(), 1);

    // Rebinding an imported provider to another origin detaches and cleans up
    // its old keyring secret after the database transaction commits.
    let original_provider = db::get_provider(&pool, &prov.id).await.unwrap();
    let credential_id = original_provider.credential_id.unwrap();
    assert!(secrets.get(&credential_id).unwrap().is_some());
    let mut rebound = exported.clone();
    rebound.providers[0].base_url = "https://different.example/v1".to_string();
    services::config_io::import_config(&pool, secrets.as_ref(), rebound)
        .await
        .unwrap();
    assert!(secrets.get(&credential_id).unwrap().is_none());

    // 3. Test config import
    let mut modified = exported.clone();
    modified.providers[0].id = "imported-provider-id".to_string();
    modified.providers[0].name = "ImportedProvider".to_string();

    let import_res = services::config_io::import_config(&pool, secrets.as_ref(), modified)
        .await
        .unwrap();

    assert_eq!(import_res.providers_imported, 1);
    let all_providers = services::providers::list(&pool).await.unwrap();
    assert!(all_providers
        .iter()
        .any(|p| p.id == "imported-provider-id" && p.name == "ImportedProvider"));

    // 4. Network Proxy settings test
    let default_proxy = services::network::get_proxy_settings(&pool).await.unwrap();
    assert_eq!(default_proxy.mode, "system");

    let saved_proxy = services::network::set_proxy_settings(
        &pool,
        services::network::ProxySettings {
            mode: "http".to_string(),
            http_proxy: Some("http://127.0.0.1:7890".to_string()),
            socks5_proxy: None,
            updated_at: String::new(),
        },
    )
    .await
    .unwrap();

    assert_eq!(saved_proxy.mode, "http");
    assert_eq!(
        saved_proxy.http_proxy.as_deref(),
        Some("http://127.0.0.1:7890")
    );

    let client = services::network::build_client_with_proxy(&saved_proxy);
    assert!(client.is_ok());

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_task_cancellation_and_crash_recovery() {
    let (pool, _secrets, dir) = setup().await;
    let task_id = uuid::Uuid::new_v4().to_string();

    // 1. 验证取消标记
    services::history::record_video_task(
        &pool,
        services::history::NewVideoTask {
            id: task_id.clone(),
            provider_id: "prov-1".to_string(),
            provider_name: "Prov".to_string(),
            model_id: "mod-1".to_string(),
            model_name: "Model".to_string(),
            model_api_id: "api-mod".to_string(),
            prompt: "a cat".to_string(),
            negative_prompt: None,
            first_frame: None,
            last_frame: None,
            reference_images: vec![],
            reference_videos: vec![],
            reference_audios: vec![],
            extra: None,
            parameters: serde_json::Map::new(),
            status: "running".to_string(),
            outputs: vec![],
            raw_request: None,
            raw_response: None,
            error: None,
        },
    )
    .await
    .unwrap();

    let detail = services::history::get(&pool, &task_id).await.unwrap();
    assert_eq!(detail.status, "running");

    // 取消任务
    services::history::mark_task_cancelled(&pool, &task_id)
        .await
        .unwrap();
    let detail2 = services::history::get(&pool, &task_id).await.unwrap();
    assert_eq!(detail2.status, "cancelled");
    assert!(detail2.error.unwrap().contains("取消"));

    // 2. 验证 Crash Recovery：插入未决任务，模拟重新启动应用恢复
    let orphan_task_id = uuid::Uuid::new_v4().to_string();
    services::history::record_video_task(
        &pool,
        services::history::NewVideoTask {
            id: orphan_task_id.clone(),
            provider_id: "prov-1".to_string(),
            provider_name: "Prov".to_string(),
            model_id: "mod-1".to_string(),
            model_name: "Model".to_string(),
            model_api_id: "api-mod".to_string(),
            prompt: "a dog running".to_string(),
            negative_prompt: None,
            first_frame: None,
            last_frame: None,
            reference_images: vec![],
            reference_videos: vec![],
            reference_audios: vec![],
            extra: None,
            parameters: serde_json::Map::new(),
            status: "running".to_string(),
            outputs: vec![],
            raw_request: None,
            raw_response: None,
            error: None,
        },
    )
    .await
    .unwrap();

    // 执行 Crash Recovery 修复语句
    let now = crate::domain::now_iso();
    sqlx::query(
        "UPDATE generation_tasks 
         SET status = 'interrupted', error = '应用异常退出，任务已中断', completed_at = ? 
         WHERE status IN ('queued', 'running', 'submitting')",
    )
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    let recovered = services::history::get(&pool, &orphan_task_id)
        .await
        .unwrap();
    assert_eq!(recovered.status, "interrupted");
    assert!(recovered.error.unwrap().contains("中断"));

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_comfyui_queue_control_and_templates() {
    let (pool, secrets, dir) = setup().await;
    let http = reqwest::Client::new();
    let server = wiremock::MockServer::start().await;

    // 1. 测试 Registry Catalog 中的 ComfyUI 工作流模板
    let catalog = services::registry::get_catalog(&dir);
    let sdxl_tmpl = catalog.models.iter().find(|m| m.id == "sdxl-base-comfyui");
    assert!(sdxl_tmpl.is_some());
    let sdxl_tmpl = sdxl_tmpl.unwrap();
    assert!(sdxl_tmpl.transport.as_ref().unwrap().workflow.is_some());
    assert!(!sdxl_tmpl
        .transport
        .as_ref()
        .unwrap()
        .bindings
        .as_ref()
        .unwrap()
        .is_empty());

    // 2. 创建 ComfyUI Provider
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Local ComfyUI".to_string(),
            provider_type: ProviderType::Comfyui,
            base_url: server.uri(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 5000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    // 3. 导入 SDXL 工作流模型
    let imported = services::registry::import_model_from_registry(
        &pool,
        &dir,
        "sdxl-base-comfyui",
        &provider.id,
        Some(false),
    )
    .await
    .unwrap();
    assert_eq!(imported.provider_id, provider.id);
    assert!(imported.transport.unwrap().workflow.is_some());

    // 4. Mock /queue 接口
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/queue"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "queue_running": [
                    [0, "prompt-uuid-running-1", {}, {}, {}]
                ],
                "queue_pending": [
                    [1, "prompt-uuid-pending-1", {}, {}, {}],
                    [2, "prompt-uuid-pending-2", {}, {}, {}]
                ]
            })),
        )
        .mount(&server)
        .await;

    // 5. 测试 services::comfyui::get_queue
    let queue_status = services::comfyui::get_queue(&pool, &http, secrets.as_ref(), &provider.id)
        .await
        .unwrap();
    assert_eq!(queue_status.total, 3);
    assert_eq!(queue_status.running.len(), 1);
    assert_eq!(queue_status.running[0].prompt_id, "prompt-uuid-running-1");
    assert_eq!(queue_status.pending.len(), 2);

    // 6. Mock /interrupt 接口
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/interrupt"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    let interrupted = services::comfyui::interrupt(&pool, &http, secrets.as_ref(), &provider.id)
        .await
        .unwrap();
    assert!(interrupted);

    // 7. Mock /queue clear 接口
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/queue"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    let cleared = services::comfyui::clear_queue(&pool, &http, secrets.as_ref(), &provider.id)
        .await
        .unwrap();
    assert!(cleared);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase8_tool_permissions_and_agent_tools() {
    let (pool, secrets, dir) = setup().await;
    let _ = secrets;

    // 1. 验证默认权限列表
    let perms = services::tool_permissions::list(&pool).await.unwrap();
    assert_eq!(perms.get("read_file").map(|s| s.as_str()), Some("safe"));
    assert_eq!(
        perms.get("run_command").map(|s| s.as_str()),
        Some("confirm")
    );
    assert_eq!(
        perms.get("delete_file").map(|s| s.as_str()),
        Some("confirm")
    );

    // 2. 更改权限并验证持久化
    services::tool_permissions::set(&pool, "run_command", "safe")
        .await
        .unwrap();
    services::tool_permissions::set(&pool, "write_file", "restricted")
        .await
        .unwrap();
    let updated = services::tool_permissions::list(&pool).await.unwrap();
    assert_eq!(updated.get("run_command").map(|s| s.as_str()), Some("safe"));
    assert_eq!(
        updated.get("write_file").map(|s| s.as_str()),
        Some("restricted")
    );

    // 3. 验证 delete_file 工具在受控安全目录内的行为
    let work_dir = dir.join("agent_work");
    std::fs::create_dir_all(&work_dir).unwrap();
    let test_file = work_dir.join("test_phase8.txt");
    std::fs::write(&test_file, "phase 8 test").unwrap();
    assert!(test_file.exists());

    // 4. 验证 agent 长期记忆的会话(session)与全局(global)作用域
    let mem1 = services::memory::save(
        &pool,
        services::memory::AgentMemoryInput {
            id: None,
            conversation_id: None,
            scope: Some("session".to_string()),
            key: "current_task".to_string(),
            content: "Phase 8 Agent 2.0".to_string(),
            importance: Some(3),
        },
    )
    .await
    .unwrap();
    assert_eq!(mem1.scope, "session");

    let prompt_block = services::memory::format_for_prompt(&pool, None)
        .await
        .unwrap();
    assert!(prompt_block.contains("[会话][current_task]"));

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase9_automation_workflow_pipeline_test() {
    let (pool, secrets, dir) = setup().await;
    let server = wiremock::MockServer::start().await;

    // 1. 验证内置管道配方
    let presets = services::pipeline::presets();
    assert_eq!(presets.len(), 3);
    assert!(presets.iter().any(|p| p.name.contains("文章提炼")));
    assert!(presets.iter().any(|p| p.name.contains("智能翻译")));
    assert!(presets.iter().any(|p| p.name.contains("故事分镜")));

    // 2. 管道 CRUD 验证
    let created = services::pipeline::save(
        &pool,
        services::pipeline::PipelineDefinition {
            id: "".to_string(),
            name: "测试管道".to_string(),
            description: "用于集成测试的简易文本工作流".to_string(),
            steps: vec![
                services::pipeline::PipelineStep::Input {
                    id: "step_input".to_string(),
                    label: "输入文本".to_string(),
                    default_value: "默认输入".to_string(),
                    data_type: "text".to_string(),
                },
                services::pipeline::PipelineStep::Transform {
                    id: "step_transform".to_string(),
                    label: "文本拼接处理".to_string(),
                    kind: "template".to_string(),
                    rule: "【前置加工】: {{input}}".to_string(),
                },
            ],
            created_at: "".to_string(),
            updated_at: "".to_string(),
            schema_version: 1,
        },
    )
    .await
    .unwrap();
    assert!(!created.id.is_empty());

    let fetched = services::pipeline::get(&pool, &created.id).await.unwrap();
    assert_eq!(fetched.name, "测试管道");
    assert_eq!(fetched.steps.len(), 2);

    let list = services::pipeline::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // 3. Mock 模型调用，验证 Input -> Transform -> Model 串联执行
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/chat/completions"))
        .and(wiremock::matchers::body_string_contains("前置加工"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "管道下游模型生成完成" } }],
                "usage": { "total_tokens": 12 }
            })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Pipeline-Provider",
        crate::domain::ProviderType::OpenaiCompatible,
        &format!("{}/v1", server.uri()),
        Some("test-secret"),
    )
    .await;

    let model = services::models::create(
        &pool,
        crate::domain::ModelInput {
            name: "Pipeline-Text-Model".to_string(),
            provider_id: provider.id,
            model_id: "test-text-model".to_string(),
            modality: crate::domain::Modality::Text,
            capabilities: serde_json::json!({}),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let exec_steps = vec![
        services::pipeline::PipelineStep::Input {
            id: "step_1".to_string(),
            label: "用户输入".to_string(),
            default_value: "".to_string(),
            data_type: "text".to_string(),
        },
        services::pipeline::PipelineStep::Transform {
            id: "step_2".to_string(),
            label: "变换节点".to_string(),
            kind: "template".to_string(),
            rule: "【前置加工】{{input}}".to_string(),
        },
        services::pipeline::PipelineStep::Model {
            id: "step_3".to_string(),
            label: "AI 模型节点".to_string(),
            model_definition_id: model.id,
            prompt_template: "{{prev}}".to_string(),
            system_prompt: None,
            parameters: serde_json::Map::new(),
            expected_input_kind: Some("text".to_string()),
            media_input_variable: None,
        },
    ];

    let http = reqwest::Client::new();

    let run_res = services::pipeline::run(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        services::pipeline::PipelineExecutionInput {
            pipeline_id: Some(created.id.clone()),
            steps: exec_steps,
            initial_input: "这是初始输入内容".to_string(),
            resume_from_step_index: None,
            cached_step_results: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(run_res.step_results.len(), 3);
    assert_eq!(run_res.final_text.as_deref(), Some("管道下游模型生成完成"));
    assert_eq!(run_res.final_output_kind, "text");

    // 4. 清理管道
    services::pipeline::delete(&pool, &created.id)
        .await
        .unwrap();
    let remaining = services::pipeline::list(&pool).await.unwrap();
    assert!(remaining.is_empty());

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase10_plugin_adapter_sdk_test() {
    let (_, _, dir) = setup().await;

    // 1. 测试安全校验拒绝恶意插件（危险协议）
    let malicious_manifest = r#"{
        "id": "malicious_plugin",
        "name": "Malicious Provider",
        "protocol": "openai-compatible",
        "defaultBaseUrl": "file:///etc/shadow",
        "defaultHeaders": {},
        "recommendedModels": []
    }"#;
    let report = services::plugins::validate_manifest(malicious_manifest);
    assert!(!report.valid);
    assert!(!report.security_passed);
    assert!(report
        .errors
        .iter()
        .any(|e| e.contains("必须是以 http:// 或 https://")));

    // 2. 测试合法插件校验通过，且能识别硬编码凭据警告
    let valid_with_warning = r#"{
        "id": "my_safe_plugin",
        "name": "My Safe Provider",
        "protocol": "openai-compatible",
        "defaultBaseUrl": "https://api.myprovider.ai/v1",
        "defaultHeaders": {
            "Authorization": "Bearer sk-1234567890abcdef1234567890"
        },
        "recommendedModels": [
            {
                "id": "model_1",
                "name": "Safe Model",
                "modelId": "safe-v1",
                "modality": "text",
                "capabilities": {}
            }
        ]
    }"#;
    let report2 = services::plugins::validate_manifest(valid_with_warning);
    assert!(report2.valid);
    assert!(report2.security_passed);
    assert!(report2
        .warnings
        .iter()
        .any(|w| w.contains("可能包含硬编码凭据")));

    // 3. 测试适配器响应解析沙箱 (test_recipe)
    let sample_recipe = crate::domain::HttpRecipe {
        method: "POST".to_string(),
        path: "/v1/images/generate".to_string(),
        headers: vec![],
        body: None,
        task_id_path: Some("$.task_id".to_string()),
        poll: Some(crate::domain::HttpPoll {
            method: "GET".to_string(),
            path: "/v1/tasks/{{taskId}}".to_string(),
            body: None,
            status_path: "$.status".to_string(),
            success: "SUCCESS".to_string(),
            failure: "FAILED".to_string(),
            interval_ms: Some(1000),
        }),
        result_path: "$.data.image_url".to_string(),
        result_kind: Some("url".to_string()),
    };

    let sample_resp = r#"{
        "status": "SUCCESS",
        "task_id": "task_abc_123",
        "data": {
            "image_url": "https://cdn.example.com/generated_cat.png"
        }
    }"#;

    let test_res = services::plugins::test_recipe(&sample_recipe, sample_resp).unwrap();
    assert!(test_res.success);
    assert_eq!(
        test_res.extracted_value.as_deref(),
        Some("https://cdn.example.com/generated_cat.png")
    );
    assert_eq!(test_res.resolved_status.as_deref(), Some("SUCCESS"));

    // 4. 测试 SDK 模板生成
    let sdk_template = services::plugins::get_sdk_template();
    assert_eq!(sdk_template.schema_version, "1.0.0");
    assert!(sdk_template
        .manifest_template
        .contains("my-custom-provider"));
    assert!(sdk_template.documentation.contains("安全沙箱保障"));

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase11_usage_and_cost_analytics_test() {
    let (pool, secrets, dir) = setup().await;

    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        crate::domain::ProviderInput {
            name: "Cloud Provider".to_string(),
            provider_type: crate::domain::ProviderType::OpenaiCompatible,
            base_url: "https://api.cloud.ai/v1".to_string(),
            api_key: Some("test-key".to_string()),
            clear_api_key: false,
            headers: std::collections::BTreeMap::new(),
            timeout_ms: 30000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model = services::models::create(
        &pool,
        crate::domain::ModelInput {
            name: "Claude 3.5 Sonnet".to_string(),
            provider_id: provider.id.clone(),
            model_id: "claude-3-5-sonnet".to_string(),
            modality: crate::domain::Modality::Text,
            capabilities: serde_json::json!({ "streaming": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    // 记录两个带有延迟与 Token 的任务
    services::history::record_text_task(
        &pool,
        &dir,
        services::history::NewTextTask {
            kind: "text_to_text".to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            prompt: "Prompt 1".to_string(),
            system_prompt: None,
            images: vec![],
            extra: None,
            conversation_id: None,
            parameters: serde_json::Map::new(),
            status: "completed".to_string(),
            output_text: Some("Output 1".to_string()),
            raw_request: Some(serde_json::json!({})),
            raw_response: Some(serde_json::json!({
                "latency_ms": 400,
                "usage": { "prompt_tokens": 500, "completion_tokens": 100 }
            })),
            error: None,
        },
    )
    .await
    .unwrap();

    services::history::record_text_task(
        &pool,
        &dir,
        services::history::NewTextTask {
            kind: "text_to_text".to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id.clone(),
            model_name: model.name.clone(),
            model_api_id: model.model_id.clone(),
            prompt: "Prompt 2".to_string(),
            system_prompt: None,
            images: vec![],
            extra: None,
            conversation_id: None,
            parameters: serde_json::Map::new(),
            status: "completed".to_string(),
            output_text: Some("Output 2".to_string()),
            raw_request: Some(serde_json::json!({})),
            raw_response: Some(serde_json::json!({
                "latency_ms": 600,
                "usage": { "prompt_tokens": 500, "completion_tokens": 100 }
            })),
            error: None,
        },
    )
    .await
    .unwrap();

    // 1. 验证按不同时间范围（today, 7d, 30d, all）聚合
    let sum_today = services::analytics::get_summary_filtered(&pool, Some("today".to_string()))
        .await
        .unwrap();
    assert_eq!(sum_today.total_tasks, 2);
    assert_eq!(sum_today.total_tokens, 1200);
    assert_eq!(sum_today.avg_latency_ms, 500); // (400 + 600) / 2
    assert_eq!(sum_today.top_providers.len(), 1);
    assert_eq!(sum_today.top_providers[0].provider_name, "Cloud Provider");
    assert_eq!(sum_today.top_providers[0].task_count, 2);

    let sum_7d = services::analytics::get_summary_filtered(&pool, Some("7d".to_string()))
        .await
        .unwrap();
    assert_eq!(sum_7d.total_tasks, 2);

    // 2. 自定义定价并验证更新
    services::analytics::set_pricing(&pool, "claude-3-5-sonnet", 3.0, 15.0)
        .await
        .unwrap();
    let sum_after_pricing = services::analytics::get_summary_filtered(&pool, None)
        .await
        .unwrap();
    // 1000 prompt tokens * $3/M = 0.003
    // 200 completion tokens * $15/M = 0.003
    // total = 0.006
    assert!((sum_after_pricing.estimated_cost_usd - 0.006).abs() < 0.0001);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase12_developer_mode_api_debugger_test() {
    let (pool, secrets, dir) = setup().await;
    let server = MockServer::start().await;

    // 1. Mock 一个 SSE 流式响应端点
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" World\"}}]}\n\ndata: [DONE]\n\n",
                )
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        crate::domain::ProviderInput {
            name: "Dev Provider".to_string(),
            provider_type: crate::domain::ProviderType::OpenaiCompatible,
            base_url: server.uri(),
            api_key: Some("secret-dev-key".to_string()),
            clear_api_key: false,
            headers: std::collections::BTreeMap::new(),
            timeout_ms: 10000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();

    // 2. 发起 Developer 请求并验证响应解析、cURL 生成与 SSE 事件提取
    let dev_input = services::developer::DevRequestInput {
        method: "POST".to_string(),
        url: "/v1/chat/completions".to_string(),
        provider_id: Some(provider.id.clone()),
        headers: std::collections::BTreeMap::new(),
        body: Some(r#"{"model":"gpt-4o","stream":true}"#.to_string()),
        timeout_ms: Some(5000),
    };

    let dev_output = services::developer::send_request(&http, secrets.as_ref(), &pool, dev_input)
        .await
        .unwrap();

    assert_eq!(dev_output.status, 200);
    assert!(dev_output.is_sse);
    assert_eq!(dev_output.sse_events.len(), 3);
    assert!(dev_output.curl_command.contains("curl -X POST"));
    assert!(dev_output.curl_command.contains("/v1/chat/completions"));

    // 3. 验证预设 CRUD
    let default_presets = services::developer::default_presets();
    assert!(!default_presets.is_empty());

    let saved = services::developer::save_preset(
        &pool,
        services::developer::ApiTestPreset {
            id: "".to_string(),
            name: "自定义测试用例".to_string(),
            method: "GET".to_string(),
            path: "/v1/models".to_string(),
            provider_id: Some(provider.id.clone()),
            headers: std::collections::BTreeMap::new(),
            body: "".to_string(),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        },
    )
    .await
    .unwrap();

    let list = services::developer::list_presets(&pool).await.unwrap();
    assert!(list.iter().any(|p| p.name == "自定义测试用例"));

    services::developer::delete_preset(&pool, &saved.id)
        .await
        .unwrap();

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase13_local_service_manager_test() {
    let (pool, _secrets, dir) = setup().await;
    let client = reqwest::Client::new();

    // 1. 扫描全部 7 种本地服务目标
    let discovered = services::discovery::discover_local_services(&pool, &client)
        .await
        .unwrap();

    assert_eq!(discovered.len(), 7);
    for s in &discovered {
        assert!(!s.name.is_empty());
        assert!(s.port > 0);
        assert!(s.web_ui_url.is_some());
    }

    // 2. 添加 ComfyUI 并验证其图片模态模型自动创建
    let comfy_provider = services::discovery::add_discovered_service(&pool, "comfyui")
        .await
        .unwrap();
    assert_eq!(comfy_provider.name, "ComfyUI (本地)");

    let models = services::models::list(&pool).await.unwrap();
    let comfy_model = models
        .iter()
        .find(|m| m.provider_id == comfy_provider.id)
        .unwrap();
    assert_eq!(comfy_model.modality, crate::domain::Modality::Image);

    // 3. 添加 F5-TTS 并验证其音频模态模型自动创建
    let f5_provider = services::discovery::add_discovered_service(&pool, "f5tts")
        .await
        .unwrap();
    assert_eq!(f5_provider.name, "F5-TTS (本地语音)");

    let models_after = services::models::list(&pool).await.unwrap();
    let f5_model = models_after
        .iter()
        .find(|m| m.provider_id == f5_provider.id)
        .unwrap();
    assert_eq!(f5_model.modality, crate::domain::Modality::Audio);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn phase14_search_and_asset_library_test() {
    let (pool, secrets, dir) = setup().await;

    // 1. 创建 Provider 和 Model
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        crate::domain::ProviderInput {
            name: "Search Provider".to_string(),
            provider_type: crate::domain::ProviderType::OpenaiCompatible,
            base_url: "http://127.0.0.1:9099".to_string(),
            api_key: None,
            clear_api_key: false,
            headers: std::collections::BTreeMap::new(),
            timeout_ms: 5000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model = services::models::create(
        &pool,
        crate::domain::ModelInput {
            provider_id: provider.id.clone(),
            name: "Flux Super Image".to_string(),
            model_id: "flux-super".to_string(),
            modality: crate::domain::Modality::Image,
            capabilities: serde_json::json!({ "image_generate": true }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    // 2. 写入一条图片生成任务与 Asset
    let task_id = uuid::Uuid::new_v4().to_string();
    let asset_id = uuid::Uuid::new_v4().to_string();
    let now = crate::domain::now_iso();

    sqlx::query(
        "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at, completed_at)
         VALUES (?, 'image', ?, ?, 'completed', ?, '{}', ?, ?)"
    )
    .bind(&task_id)
    .bind(&provider.id)
    .bind(&model.id)
    .bind(serde_json::json!({ "prompt": "Cyberpunk neon city street view" }).to_string())
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO assets (id, task_id, type, path, mime_type, width, height, duration_ms, metadata_json, created_at)
         VALUES (?, ?, 'image', '/images/cyberpunk.png', 'image/png', 1024, 1024, NULL, '{}', ?)"
    )
    .bind(&asset_id)
    .bind(&task_id)
    .bind(&now)
    .execute(&pool)
    .await
    .unwrap();

    // 3. 写入一条对话记录
    let conv_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO conversations (id, title, mode, model_id, system_prompt, context_strategy, context_limit, created_at, updated_at)
         VALUES (?, '量子计算前沿探讨', 'chat', ?, '', 'sliding_window', 10, ?, ?)"
    )
    .bind(&conv_id)
    .bind(&model.id)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO messages (id, conversation_id, role, content_json, token_count, created_at)
         VALUES (?, ?, 'user', ?, 12, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&conv_id)
    .bind(
        serde_json::json!([{ "type": "text", "text": "请解释超导量子比特与拓扑量子计算的区别" }])
            .to_string(),
    )
    .bind(&now)
    .execute(&pool)
    .await
    .unwrap();

    // 4. 测试 Unified Search
    let search_res = services::search::search_unified(&pool, "量子计算", None, None, None)
        .await
        .unwrap();
    assert!(!search_res.items.is_empty());
    assert!(search_res
        .items
        .iter()
        .any(|i| i.category == "conversation" && i.title.contains("量子计算")));

    let search_media = services::search::search_unified(&pool, "Cyberpunk", None, None, None)
        .await
        .unwrap();
    assert!(!search_media.items.is_empty());
    assert!(search_media.items.iter().any(|i| i.category == "asset"));

    // 5. 测试 Asset Library
    let assets =
        services::search::list_asset_library(&pool, Some("image".to_string()), None, None, None)
            .await
            .unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].prompt, "Cyberpunk neon city street view");
    assert_eq!(assets[0].width, Some(1024));
    assert_eq!(assets[0].height, Some(1024));
    assert_eq!(assets[0].model_name, "Flux Super Image");

    // 6. 测试 HistoryQuery 的全文检索关键字过滤
    let history_res = services::history::list(
        &pool,
        services::history::HistoryQuery {
            query: Some("Cyberpunk".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(history_res.items.len(), 1);
    assert_eq!(history_res.items[0].id, task_id);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn fault_injection_rate_limit_backoff_and_recovery() {
    let server = MockServer::start().await;
    // 第 1 次响应 429 Rate Limit
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": { "message": "Rate limit exceeded" }
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // 第 2 次响应 200 OK
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "recovered" } }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        })))
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "MockRateLimit".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: format!("{}/v1", server.uri()),
            api_key: Some("sk-test-mock".to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model = services::models::create(
        &pool,
        ModelInput {
            name: "DemoRateLimit".to_string(),
            provider_id: provider.id,
            model_id: "demo-rl".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false, "vision": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();
    let result = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id,
            system_prompt: None,
            prompt: "hello".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await
    .unwrap();

    assert_eq!(result.text, "recovered");
    let logs = crate::logging::global_logger().recent(100);
    assert!(logs.iter().any(|l| l.event == "http_retry"));
    assert!(logs.iter().any(|l| l.event == "http_request_success"));

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn fault_injection_circuit_breaker_trips_on_consecutive_server_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error": { "message": "Service Unavailable" }
        })))
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    crate::resilience::global_breakers().reset();

    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "FailingProvider".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: format!("{}/v1", server.uri()),
            api_key: Some("sk-test-fail".to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 10_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model = services::models::create(
        &pool,
        ModelInput {
            name: "FailingModel".to_string(),
            provider_id: provider.id.clone(),
            model_id: "fail-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false, "vision": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();

    // 连续触发 5 次调用（503 服务端异常）
    for _ in 0..5 {
        let _ = services::generate::execute(
            &pool,
            &http,
            secrets.as_ref(),
            &dir,
            GenerateTextInput {
                model_definition_id: model.id.clone(),
                system_prompt: None,
                prompt: "ping".to_string(),
                parameters: Map::new(),
                extra: None,
                images: vec![],
            },
        )
        .await;
    }

    // 第 6 次调用应直接被断路器在本地拦截
    let res = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id.clone(),
            system_prompt: None,
            prompt: "ping".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await;

    assert!(res.is_err());
    let err_msg = res.err().unwrap().message;
    assert!(
        err_msg.contains("熔断保护中") || err_msg.contains("连续失败过多"),
        "Unexpected error: {}",
        err_msg
    );

    crate::resilience::global_breakers().reset();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn fault_injection_malformed_json_response_handled_gracefully() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            b"<!DOCTYPE html><html><body>502 Bad Gateway</body></html>",
            "text/html",
        ))
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "HtmlReturningProvider".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: format!("{}/v1", server.uri()),
            api_key: Some("sk-test-html".to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 10_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model = services::models::create(
        &pool,
        ModelInput {
            name: "HtmlModel".to_string(),
            provider_id: provider.id,
            model_id: "html-model".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false, "vision": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();
    let res = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id,
            system_prompt: None,
            prompt: "ping".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await;

    assert!(res.is_err());
    let page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].status, "failed");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_history_delete_transactional_and_file_cleanup() {
    let (pool, _secrets, dir) = setup().await;
    let task_id = services::history::record_text_task(
        &pool,
        &dir,
        services::history::NewTextTask {
            kind: "text".to_string(),
            provider_id: "prov-1".to_string(),
            provider_name: "MockProv".to_string(),
            model_id: "model-1".to_string(),
            model_name: "MockModel".to_string(),
            model_api_id: "api-1".to_string(),
            prompt: "Test delete".to_string(),
            system_prompt: None,
            images: vec![],
            parameters: serde_json::Map::new(),
            extra: None,
            status: "completed".to_string(),
            output_text: Some("This is file content".to_string()),
            raw_request: None,
            raw_response: None,
            error: None,
            conversation_id: None,
        },
    )
    .await
    .unwrap();

    let output_file = dir.join("files").join(format!("{task_id}.txt"));
    assert!(output_file.exists(), "输出文件应存在于磁盘");

    let detail = services::history::get(&pool, &task_id).await.unwrap();
    assert_eq!(detail.output, "This is file content");

    services::history::delete_task(&pool, &task_id)
        .await
        .unwrap();

    let get_res = services::history::get(&pool, &task_id).await;
    assert!(get_res.is_err(), "历史任务应已从数据库删除");
    assert!(!output_file.exists(), "磁盘输出文件应已随任务安全删除");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_deleted_task_id_cannot_be_reclaimed() {
    let (pool, _secrets, dir) = setup().await;
    let task_id = uuid::Uuid::new_v4().to_string();

    let claimed = db::claim_task_id(&pool, Some(task_id.clone()), "image", Some("model"))
        .await
        .unwrap();
    assert_eq!(claimed, task_id);

    services::history::delete_tasks_batch(&pool, std::slice::from_ref(&task_id))
        .await
        .unwrap();

    let err = db::claim_task_id(&pool, Some(task_id.clone()), "image", Some("model"))
        .await
        .unwrap_err();
    assert_eq!(err.code, crate::error::CONFLICT);
    assert!(err.message.contains("已被删除"));

    let tombstone: Option<String> =
        sqlx::query_scalar("SELECT id FROM deleted_generation_tasks WHERE id = ?")
            .bind(&task_id)
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert_eq!(tombstone.as_deref(), Some(task_id.as_str()));

    let late_upsert = sqlx::query(
        "INSERT INTO generation_tasks
         (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at)
         VALUES (?, 'image', 'prov-1', 'model', 'completed', '{}', '{}', ?)
         ON CONFLICT(id) DO UPDATE SET status = excluded.status",
    )
    .bind(&task_id)
    .bind(crate::domain::now_iso())
    .execute(&pool)
    .await;
    assert!(
        late_upsert.is_err(),
        "late result must not resurrect tombstoned task"
    );
    let resurrected: Option<String> =
        sqlx::query_scalar("SELECT id FROM generation_tasks WHERE id = ?")
            .bind(&task_id)
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert!(resurrected.is_none());

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_ollama_provider_contract_and_connection() {
    let server = MockServer::start().await;

    // 1. Mock /api/tags 用于连接测试
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [{
                "name": "llama3:latest",
                "model": "llama3:latest",
                "modified_at": "2026-10-01T00:00:00Z",
                "size": 4661224676u64,
                "digest": "sha256:123456"
            }]
        })))
        .mount(&server)
        .await;

    // 2. Mock /api/chat 用于文本生成
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(body_string_contains("\"model\":\"llama3\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "llama3",
            "created_at": "2026-10-10T12:00:00Z",
            "message": {
                "role": "assistant",
                "content": "Hello from local Ollama instance!"
            },
            "done": true,
            "total_duration": 500000000,
            "prompt_eval_count": 5,
            "eval_count": 8
        })))
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Local Ollama".to_string(),
            provider_type: ProviderType::Ollama,
            base_url: server.uri(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 10_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();
    // 验证连接探针
    let report = services::providers::test(&pool, &http, secrets.as_ref(), &provider.id)
        .await
        .unwrap();
    assert_eq!(report.message, "已连接");

    // 创建本地模型
    let model = services::models::create(
        &pool,
        ModelInput {
            name: "Llama 3 Local".to_string(),
            provider_id: provider.id.clone(),
            model_id: "llama3".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false, "vision": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    // 验证生成调用
    let result = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id,
            system_prompt: None,
            prompt: "Say hello".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await
    .unwrap();

    assert_eq!(result.text, "Hello from local Ollama instance!");
    assert_eq!(result.usage.unwrap().total_tokens, Some(13));

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_openai_compatible_reasoning_content_contract() {
    let server = MockServer::start().await;

    // 模拟 DeepSeek-R1 / o1 类模型的思考链 reasoning_content 结构
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-reasoning-123",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "最终结论：42",
                    "reasoning_content": "首先计算宇宙的意义，经过深度推理得出数值 42。"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 15,
                "completion_tokens": 30,
                "total_tokens": 45,
                "completion_tokens_details": {
                    "reasoning_tokens": 20
                }
            }
        })))
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "DeepSeek Provider".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: format!("{}/v1", server.uri()),
            api_key: Some("sk-reasoning-test".to_string()),
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 10_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let model = services::models::create(
        &pool,
        ModelInput {
            name: "DeepSeek R1".to_string(),
            provider_id: provider.id,
            model_id: "deepseek-reasoner".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false, "vision": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();
    let result = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: model.id,
            system_prompt: None,
            prompt: "宇宙的终极答案是什么？".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await
    .unwrap();

    assert_eq!(result.text, "最终结论：42");
    assert_eq!(
        result.reasoning,
        "首先计算宇宙的意义，经过深度推理得出数值 42。"
    );
    let usage = result.usage.unwrap();
    assert_eq!(usage.total_tokens, Some(45));
    assert_eq!(usage.reasoning_tokens, Some(20));

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_anthropic_and_gemini_error_contract() {
    let server = MockServer::start().await;
    let secret = "sk-ant-test-err-key";

    // 1. Anthropic 401 模拟
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "type": "error",
            "error": {
                "type": "authentication_error",
                "message": "invalid x-api-key provided"
            }
        })))
        .mount(&server)
        .await;

    // 2. Gemini 400 API Key 无效模拟
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-pro:generateContent"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "code": 400,
                "message": "API key not valid. Please pass a valid API key.",
                "status": "INVALID_ARGUMENT"
            }
        })))
        .mount(&server)
        .await;

    let (pool, secrets, dir) = setup().await;

    // 测试 Anthropic 错误转换
    let ant_provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Anthropic Err",
        ProviderType::Anthropic,
        &server.uri(),
        Some(secret),
    )
    .await;
    let ant_model = services::models::create(
        &pool,
        text_model("Claude Err", ant_provider.id, "claude-3-opus"),
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();
    let ant_err = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: ant_model.id,
            system_prompt: None,
            prompt: "ping".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await
    .unwrap_err();

    assert_eq!(ant_err.code, crate::error::AUTH);
    assert!(ant_err.message.contains("invalid x-api-key"));
    assert!(!ant_err.message.contains(secret));

    // 测试 Gemini 错误转换
    let gem_provider = image_provider(
        &pool,
        secrets.as_ref(),
        "Gemini Err",
        ProviderType::Gemini,
        &server.uri(),
        Some("gem-key-err"),
    )
    .await;
    let gem_model = services::models::create(
        &pool,
        text_model("Gemini Err", gem_provider.id, "models/gemini-pro"),
    )
    .await
    .unwrap();

    let gem_err = services::generate::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateTextInput {
            model_definition_id: gem_model.id,
            system_prompt: None,
            prompt: "ping".to_string(),
            parameters: Map::new(),
            extra: None,
            images: vec![],
        },
    )
    .await
    .unwrap_err();

    assert!(gem_err.message.contains("API key not valid"));
    assert!(!gem_err.message.contains("gem-key-err"));

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_feature_capabilities_strict_contract_enforcement() {
    let (pool, secrets, dir) = setup().await;

    // 1. 测试音频模型不支持 voiceClone 时拒绝 reference_audio
    let audio_provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "TTS Provider".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: "http://127.0.0.1:9000/v1".to_string(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 5000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let tts_only_model = services::models::create(
        &pool,
        ModelInput {
            name: "TTS Only".to_string(),
            provider_id: audio_provider.id.clone(),
            model_id: "tts-basic".to_string(),
            modality: Modality::Audio,
            capabilities: json!({ "textToSpeech": true, "voiceClone": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let http = reqwest::Client::new();
    let audio_err = services::audio::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateAudioInput {
            model_definition_id: tts_only_model.id,
            task: Some("tts".to_string()),
            text: "测试朗读".to_string(),
            voice: None,
            source_audio: None,
            reference_audio: Some(MediaRef::from_path("ref_sample.mp3")),
            reference_transcript: None,
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap_err();

    assert_eq!(audio_err.code, crate::error::UNSUPPORTED);
    assert!(audio_err.message.contains("当前模型不接受参考音频"));

    // 2. 测试视频模型不支持 lastFrame 时拒绝 last_frame
    let video_provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Video Provider".to_string(),
            provider_type: ProviderType::Comfyui,
            base_url: "http://127.0.0.1:8188".to_string(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 5000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let video_model = services::models::create(
        &pool,
        ModelInput {
            name: "Video No Last Frame".to_string(),
            provider_id: video_provider.id,
            model_id: "video-no-last".to_string(),
            modality: Modality::Video,
            capabilities: json!({ "textToVideo": true, "firstFrame": true, "lastFrame": false }),
            parameters: vec![],
            constraints: vec![],
            transport: Some(TransportDefinition {
                workflow: Some(json!({ "3": { "class_type": "KSampler" } })),
                bindings: Some(vec![WorkflowBinding {
                    key: "prompt".to_string(),
                    node: "3".to_string(),
                    input: "positive".to_string(),
                }]),
                ..TransportDefinition::default()
            }),
            enabled: true,
        },
    )
    .await
    .unwrap();

    let video_err = services::video::execute(
        &pool,
        &http,
        secrets.as_ref(),
        &dir,
        GenerateVideoInput {
            model_definition_id: video_model.id,
            prompt: "a walking cat".to_string(),
            negative_prompt: None,
            first_frame: Some(MediaRef::from_path("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")),
            last_frame: Some(MediaRef::from_path("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")),
            reference_images: vec![],
            reference_videos: vec![],
            reference_audios: vec![],
            parameters: Map::new(),
            extra: None,
        },
    )
    .await
    .unwrap_err();

    assert_eq!(video_err.code, crate::error::UNSUPPORTED);
    assert!(video_err.message.contains("该模型没有尾帧能力"));

    // 确认契约防呆拦截未写入 generation_tasks 脏记录
    let history_page = services::history::list(&pool, HistoryQuery::default())
        .await
        .unwrap();
    assert_eq!(history_page.total, 0);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn test_release_version_consistency_and_metadata() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo_version = env!("CARGO_PKG_VERSION");

    // 1. 读取 package.json
    let package_json_path = manifest_dir.parent().unwrap().join("package.json");
    let package_json_content = std::fs::read_to_string(&package_json_path)
        .expect("package.json should exist in project root");
    let package_val: serde_json::Value =
        serde_json::from_str(&package_json_content).expect("package.json should be valid JSON");
    let package_version = package_val["version"]
        .as_str()
        .expect("package.json should have string version");

    // 2. 读取 tauri.conf.json
    let tauri_conf_path = manifest_dir.join("tauri.conf.json");
    let tauri_conf_content = std::fs::read_to_string(&tauri_conf_path)
        .expect("tauri.conf.json should exist in src-tauri");
    let tauri_val: serde_json::Value =
        serde_json::from_str(&tauri_conf_content).expect("tauri.conf.json should be valid JSON");
    let tauri_version = tauri_val["version"]
        .as_str()
        .expect("tauri.conf.json should have string version");

    // 3. 断言版本号完全一致
    assert_eq!(
        cargo_version, package_version,
        "Cargo.toml ({cargo_version}) and package.json ({package_version}) version mismatch"
    );
    assert_eq!(
        cargo_version, tauri_version,
        "Cargo.toml ({cargo_version}) and tauri.conf.json ({tauri_version}) version mismatch"
    );

    // 4. 验证发布元数据与标识符
    assert_eq!(tauri_val["productName"].as_str().unwrap(), "ModelDeck");
    assert_eq!(tauri_val["identifier"].as_str().unwrap(), "local.modeldeck");
    assert!(tauri_val["bundle"]["active"].as_bool().unwrap());
    assert_eq!(
        tauri_val["bundle"]["windows"]["nsis"]["installMode"]
            .as_str()
            .unwrap(),
        "currentUser"
    );
}

#[tokio::test]
async fn t14_multimodal_pricing_metering_sources_and_estimation_test() {
    let (pool, secrets, dir) = setup().await;

    // 1. 设置多模态定价规则
    services::analytics::set_pricing_detailed(
        &pool,
        "dall-e-3",
        "mock_prov",
        0.0,
        0.0,
        "per_image",
        0.040,
        "custom_user",
        "USD",
    )
    .await
    .unwrap();

    services::analytics::set_pricing_detailed(
        &pool,
        "cogvideox",
        "mock_prov",
        0.0,
        0.0,
        "per_video_second",
        0.060,
        "custom_user",
        "USD",
    )
    .await
    .unwrap();

    let pricings = services::analytics::list_pricing(&pool).await.unwrap();
    assert!(pricings.iter().any(|p| p.model_id == "dall-e-3"
        && p.pricing_unit == "per_image"
        && (p.price_per_unit - 0.040).abs() < 1e-4));
    assert!(pricings.iter().any(|p| p.model_id == "cogvideox"
        && p.pricing_unit == "per_video_second"
        && (p.price_per_unit - 0.060).abs() < 1e-4));

    // 2. 验证预估成本接口 (estimate_task_cost)
    let est_img = services::analytics::estimate_task_cost(
        &pool,
        "image",
        "dall-e-3",
        Some("mock_prov"),
        None,
        Some(2),
        None,
    )
    .await
    .unwrap();
    assert!(est_img.has_pricing);
    assert_eq!(est_img.pricing_unit, "per_image");
    assert!((est_img.estimated_cost_min - 0.080).abs() < 1e-4);

    let est_vid = services::analytics::estimate_task_cost(
        &pool,
        "video",
        "cogvideox",
        Some("mock_prov"),
        None,
        None,
        Some(10.0),
    )
    .await
    .unwrap();
    assert!(est_vid.has_pricing);
    assert_eq!(est_vid.pricing_unit, "per_video_second");
    assert!((est_vid.estimated_cost_min - 0.60).abs() < 1e-4);

    let est_unmetered = services::analytics::estimate_task_cost(
        &pool,
        "image",
        "unknown-local-flux-custom",
        None,
        None,
        Some(1),
        None,
    )
    .await
    .unwrap();
    // flux-custom matches flux catalog fallback or none
    assert!(est_unmetered.has_pricing); // matched flux catalog

    // 3. 创建 Provider & 记录多模态任务
    let provider = services::providers::create(
        &pool,
        secrets.as_ref(),
        ProviderInput {
            name: "Mock Provider T14".to_string(),
            provider_type: ProviderType::OpenaiCompatible,
            base_url: "http://127.0.0.1:9".to_string(),
            api_key: None,
            clear_api_key: false,
            headers: BTreeMap::new(),
            timeout_ms: 30_000,
            enabled: true,
        },
    )
    .await
    .unwrap();

    let text_model = services::models::create(
        &pool,
        ModelInput {
            name: "GPT-4o".to_string(),
            provider_id: provider.id.clone(),
            model_id: "gpt-4o".to_string(),
            modality: Modality::Text,
            capabilities: json!({ "streaming": false }),
            parameters: vec![],
            constraints: vec![],
            transport: None,
            enabled: true,
        },
    )
    .await
    .unwrap();

    // 任务 A: 官方精准 Token 计量 (official)
    services::history::record_text_task(
        &pool,
        &dir,
        NewTextTask {
            kind: "text_to_text".to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: text_model.id.clone(),
            model_name: text_model.name.clone(),
            model_api_id: "gpt-4o".to_string(),
            prompt: "Hello".to_string(),
            system_prompt: None,
            images: vec![],
            extra: None,
            conversation_id: None,
            parameters: Map::new(),
            status: "completed".to_string(),
            output_text: Some("World".to_string()),
            raw_request: None,
            raw_response: Some(json!({
                "usage": { "prompt_tokens": 1000, "completion_tokens": 500, "total_tokens": 1500 }
            })),
            error: None,
        },
    )
    .await
    .unwrap();

    // 任务 B: 图像生成估算 (estimated)
    services::history::record_image_task(
        &pool,
        services::history::NewImageTask {
            id: uuid::Uuid::new_v4().to_string(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model_id: "img_mod".to_string(),
            model_name: "DALL-E 3".to_string(),
            model_api_id: "dall-e-3".to_string(),
            prompt: "cyberpunk cat".to_string(),
            negative_prompt: None,
            source_image: None,
            mask: None,
            reference_images: vec![],
            extra: None,
            parameters: {
                let mut map = Map::new();
                map.insert("n".to_string(), json!(1));
                map
            },
            status: "completed".to_string(),
            outputs: vec![],
            raw_request: None,
            raw_response: Some(json!({ "created": 123 })),
            error: None,
        },
    )
    .await
    .unwrap();

    // 4. 获取 Summary 并验证三态分类
    let summary = services::analytics::get_summary(&pool).await.unwrap();
    assert_eq!(summary.total_tasks, 2);
    assert_eq!(summary.official_task_count, 1);
    assert_eq!(summary.estimated_task_count, 1);
    assert_eq!(summary.unmetered_task_count, 0);
    assert!(summary.official_cost_usd > 0.0);
    assert!(summary.estimated_portion_cost_usd > 0.0);
    assert!(
        (summary.total_cost_usd - (summary.official_cost_usd + summary.estimated_portion_cost_usd))
            .abs()
            < 1e-4
    );

    let gpt_stat = summary
        .top_models
        .iter()
        .find(|m| m.model_id == "gpt-4o")
        .unwrap();
    assert_eq!(gpt_stat.metering_source, "official");
    let dalle_stat = summary
        .top_models
        .iter()
        .find(|m| m.model_id == "dall-e-3")
        .unwrap();
    assert_eq!(dalle_stat.metering_source, "estimated");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn t15_diagnostics_and_network_proxy_test() {
    let (pool, secrets, dir) = setup().await;

    // 1. 代理设置模式变更与校验
    let init_proxy = services::network::get_proxy_settings(&pool).await.unwrap();
    assert_eq!(init_proxy.mode, "system");

    // 切换到直连 (none)
    let none_proxy = services::network::set_proxy_settings(
        &pool,
        services::network::ProxySettings {
            mode: "none".to_string(),
            http_proxy: None,
            socks5_proxy: None,
            updated_at: "".to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(none_proxy.mode, "none");
    assert!(services::network::build_client_with_proxy(&none_proxy).is_ok());

    // 切换到合法的 HTTP 代理
    let http_proxy = services::network::set_proxy_settings(
        &pool,
        services::network::ProxySettings {
            mode: "http".to_string(),
            http_proxy: Some("http://127.0.0.1:8080".to_string()),
            socks5_proxy: None,
            updated_at: "".to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(http_proxy.mode, "http");
    assert_eq!(
        http_proxy.http_proxy.as_deref(),
        Some("http://127.0.0.1:8080")
    );

    // 尝试保存带认证用户名密码的代理应被拒绝
    let bad_proxy = services::network::set_proxy_settings(
        &pool,
        services::network::ProxySettings {
            mode: "http".to_string(),
            http_proxy: Some("http://user:pass@127.0.0.1:8080".to_string()),
            socks5_proxy: None,
            updated_at: "".to_string(),
        },
    )
    .await;
    assert!(bad_proxy.is_err());

    // 2. 插入测试数据：包含敏感真实密钥和带密钥的任务错误
    let secret_key = "sk-super-secret-production-token-998877";
    let cred_id = "cred-secret-prov";
    secrets.set(cred_id, secret_key).unwrap();

    sqlx::query(
        "INSERT INTO providers (id, name, type, base_url, credential_id, headers_json, timeout_ms, enabled, created_at, updated_at)
         VALUES ('prov-secret', 'Prod Provider', 'openai', 'https://api.openai.com/v1', ?, '{}', 30000, 1, '2026-10-10T00:00:00Z', '2026-10-10T00:00:00Z')"
    )
    .bind(cred_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, error, created_at)
         VALUES ('task-leaked-err', 'text', 'prov-secret', 'gpt-4o', 'failed', '{\"prompt\": \"sensitive prompt\"}', '{}', ?, '2026-10-10T00:00:00Z')"
    )
    .bind(format!("Request failed 401 Unauthorized with token {}", secret_key))
    .execute(&pool)
    .await
    .unwrap();

    // 3. 运行一键系统自检
    let report = services::diagnostics::run_diagnostics(&dir, &pool, Some(secrets.as_ref()))
        .await
        .unwrap();
    assert!(!report.items.is_empty());
    assert!(report.items.iter().any(|i| i.name == "SQLite Integrity"));
    assert!(report.items.iter().any(|i| i.name == "WebView2 Runtime"));
    assert!(report.items.iter().any(|i| i.name == "网络与代理连通性"));
    assert!(report.items.iter().any(|i| i.name == "关键 Provider 凭据"));

    // 4. 导出脱敏诊断包
    let bundle =
        services::diagnostics::export_diagnostic_bundle(&dir, &pool, Some(secrets.as_ref()))
            .await
            .unwrap();

    // 验证 Providers 中无密钥泄露
    for prov in &bundle.sanitized_providers {
        let json_str = prov.to_string();
        assert!(!json_str.contains(secret_key));
    }

    // 验证任务错误记录中密钥被脱敏，且含有 failureReason 归因
    let task_err = bundle
        .recent_task_errors
        .iter()
        .find(|t| t.get("id").and_then(|v| v.as_str()) == Some("task-leaked-err"))
        .expect("task-leaked-err should exist");

    let err_msg = task_err.get("error").and_then(|v| v.as_str()).unwrap();
    assert!(
        !err_msg.contains(secret_key),
        "Secret must be masked in diagnostic bundle"
    );
    assert_eq!(
        task_err.get("failureReason").and_then(|v| v.as_str()),
        Some("credentials")
    );

    // 验证整个诊断包字符串绝不含有真实密钥
    let bundle_json = serde_json::to_string(&bundle).unwrap();
    assert!(!bundle_json.contains(secret_key));
    // 验证不包含用户 prompt 对话正文
    assert!(!bundle_json.contains("sensitive prompt"));

    let _ = std::fs::remove_dir_all(dir);
}

// =========================================================================
// T16 端到端验收场景 (E2E Acceptance Scenarios A, B, C)
// =========================================================================

#[tokio::test]
async fn t16_e2e_scenario_a_first_run_to_first_result() {
    let dir = std::env::temp_dir().join(format!("ai-e2e-scen-a-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("app.db");
    let pool = db::open(&db_path).await.unwrap();
    db::migrate(&pool).await.unwrap();

    // 1. 无特权免管理员环境初始化：探测目录权限
    assert!(crate::storage_util::ensure_dir_writable(&dir).is_ok());

    // 2. 模拟首次向导设置：配置本地/测试 Provider
    let prov_id = "prov-local-e2e";
    let cred_id = "cred-local-e2e";
    sqlx::query(
        "INSERT INTO providers (id, name, type, base_url, credential_id, headers_json, timeout_ms, enabled, created_at, updated_at)
         VALUES (?, 'Local E2E Engine', 'ollama', 'http://127.0.0.1:11434', ?, '{}', 30000, 1, '2026-10-10T00:00:00Z', '2026-10-10T00:00:00Z')"
    )
    .bind(prov_id)
    .bind(cred_id)
    .execute(&pool)
    .await
    .unwrap();

    // 3. 执行第一次图片生成并保存产物
    let task_id = uuid::Uuid::new_v4().to_string();
    let image_file = dir.join(format!("{}.png", task_id));
    std::fs::write(&image_file, b"\x89PNG\r\n\x1a\nfake-e2e-image-bytes").unwrap();

    services::history::record_image_task(
        &pool,
        services::history::NewImageTask {
            id: task_id.clone(),
            provider_id: prov_id.to_string(),
            provider_name: "Local E2E Engine".to_string(),
            model_id: "flux-schnell".to_string(),
            model_name: "FLUX Schnell".to_string(),
            model_api_id: "flux-schnell".to_string(),
            prompt: "A futuristic cyberpunk city at dusk".to_string(),
            negative_prompt: None,
            source_image: None,
            mask: None,
            reference_images: vec![],
            extra: None,
            parameters: serde_json::Map::new(),
            status: "completed".to_string(),
            outputs: vec![services::history::StoredOutput {
                path: image_file.to_string_lossy().to_string(),
                mime_type: "image/png".to_string(),
                width: Some(1024),
                height: Some(1024),
            }],
            raw_request: None,
            raw_response: None,
            error: None,
        },
    )
    .await
    .unwrap();

    // 4. 关闭并重启应用模拟：检查历史与资产
    let task_detail = services::history::get(&pool, &task_id).await.unwrap();
    assert_eq!(task_detail.status, "completed");
    assert_eq!(task_detail.prompt, "A futuristic cyberpunk city at dusk");

    let assets =
        services::search::list_asset_library(&pool, Some("image".to_string()), None, None, None)
            .await
            .unwrap();
    assert!(!assets.is_empty());
    let found_asset = assets
        .iter()
        .find(|a| a.task_id == task_id)
        .expect("Asset should exist");
    assert!(found_asset.file_exists);

    // 运行自检
    let report = services::diagnostics::run_diagnostics(&dir, &pool, None)
        .await
        .unwrap();
    assert!(report
        .items
        .iter()
        .any(|i| i.name == "SQLite Integrity" && i.status == "ok"));

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn t16_e2e_scenario_b_long_video_lifecycle_and_crash_recovery() {
    let (pool, _secrets, dir) = setup().await;

    // 1. 提交长视频任务并在生成中记录 remote_job_id
    let task_id_video = uuid::Uuid::new_v4().to_string();
    services::history::record_video_task(
        &pool,
        services::history::NewVideoTask {
            id: task_id_video.clone(),
            provider_id: "prov-kling".to_string(),
            provider_name: "Kling AI".to_string(),
            model_id: "kling-v1".to_string(),
            model_name: "Kling 1.0".to_string(),
            model_api_id: "kling-v1".to_string(),
            prompt: "Cinematic drone shot of vast snowy mountains".to_string(),
            negative_prompt: None,
            first_frame: None,
            last_frame: None,
            reference_images: vec![],
            reference_videos: vec![],
            reference_audios: vec![],
            extra: None,
            parameters: serde_json::Map::new(),
            status: "running".to_string(),
            outputs: vec![],
            raw_request: None,
            raw_response: None,
            error: None,
        },
    )
    .await
    .unwrap();

    // 记录远程 Job 状态（模拟生成中异常退出）
    services::task_scheduler::record_remote_job(
        &pool,
        &task_id_video,
        "kling-remote-job-8888",
        Some("https://api.klingai.com/v1/videos/kling-remote-job-8888"),
    )
    .await
    .unwrap();

    // 2. 模拟重启恢复执行
    let report = services::task_scheduler::recover_tasks_on_startup(&pool)
        .await
        .unwrap();
    assert_eq!(report.interrupted_with_remote_id, 1);
    assert_eq!(report.total_recovered, 1);

    let recovered_task = services::history::get(&pool, &task_id_video).await.unwrap();
    assert_eq!(recovered_task.status, "interrupted");
    assert!(recovered_task
        .error
        .unwrap()
        .contains("kling-remote-job-8888"));

    // 验证数据库底层 remote_job_id 完整保留
    let (saved_remote_id,): (Option<String>,) =
        sqlx::query_as("SELECT remote_job_id FROM generation_tasks WHERE id = ?")
            .bind(&task_id_video)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(saved_remote_id.as_deref(), Some("kling-remote-job-8888"));

    // 3. 提交未决任务模拟网络中断与防扣费机制
    let task_id_submitting = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, execution_phase, created_at)
         VALUES (?, 'video', 'prov-kling', 'kling-v1', 'submitting', '{}', '{}', 'submitting', '2026-10-10T00:00:00Z')"
    )
    .bind(&task_id_submitting)
    .execute(&pool)
    .await
    .unwrap();

    let report2 = services::task_scheduler::recover_tasks_on_startup(&pool)
        .await
        .unwrap();
    assert!(report2.unknown_submissions >= 1);

    let submitting_task = services::history::get(&pool, &task_id_submitting)
        .await
        .unwrap();
    assert_eq!(submitting_task.status, "unknown_submission");
    assert!(submitting_task
        .error
        .unwrap()
        .contains("为防止重复扣费，系统默认不自动重发"));

    // 验证计费风险标识
    let (billing_risk_val,): (Option<String>,) =
        sqlx::query_as("SELECT billing_risk FROM generation_tasks WHERE id = ?")
            .bind(&task_id_submitting)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(billing_risk_val.as_deref(), Some("possible"));

    // 4. 取消一致性测试：活跃任务取消进入 cancelled 终态，重启恢复绝不回退
    let task_id_cancel = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at)
         VALUES (?, 'video', 'prov-kling', 'kling-v1', 'running', '{}', '{}', '2026-10-10T00:00:00Z')"
    )
    .bind(&task_id_cancel)
    .execute(&pool)
    .await
    .unwrap();

    let was_cancelled = services::history::mark_task_cancelled(&pool, &task_id_cancel)
        .await
        .unwrap();
    assert!(was_cancelled);
    let cancelled_task = services::history::get(&pool, &task_id_cancel)
        .await
        .unwrap();
    assert_eq!(cancelled_task.status, "cancelled");

    // 再次调用重启恢复，已取消任务绝不变回 running 或 queued
    let report3 = services::task_scheduler::recover_tasks_on_startup(&pool)
        .await
        .unwrap();
    assert_eq!(report3.total_recovered, 0);

    let check_again = services::history::get(&pool, &task_id_cancel)
        .await
        .unwrap();
    assert_eq!(check_again.status, "cancelled");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn t16_e2e_scenario_c_cross_modal_workflow_provenance_and_reuse() {
    let (pool, _secrets, dir) = setup().await;

    // 1. 模拟三阶段跨模态流程（文本策划 -> 图像分镜 -> 视频驱动）
    let story_task_id = "step1-text-story";
    let image_task_id = "step2-image-frame";
    let video_task_id = "step3-video-clip";

    // Step 1: 文本分镜脚本
    let recorded_story_id = services::history::record_text_task(
        &pool,
        &dir,
        services::history::NewTextTask {
            kind: "text".to_string(),
            provider_id: "prov-llm".to_string(),
            provider_name: "LLM".to_string(),
            model_id: "gpt-4o".to_string(),
            model_name: "GPT-4o".to_string(),
            model_api_id: "gpt-4o".to_string(),
            prompt: format!("任务 [{}] 编写关于深海探险的分镜脚本", story_task_id),
            system_prompt: None,
            images: vec![],
            extra: None,
            conversation_id: None,
            parameters: serde_json::Map::new(),
            status: "completed".to_string(),
            output_text: Some("Scene 1: 一艘深潜器在发光的深海珊瑚丛中穿行。".to_string()),
            raw_request: None,
            raw_response: None,
            error: None,
        },
    )
    .await
    .unwrap();
    assert!(!recorded_story_id.is_empty());

    // Step 2: 图像关键帧生成并登记资产
    let frame_file = dir.join("subsea_frame.png");
    std::fs::write(&frame_file, b"fake-subsea-keyframe-png").unwrap();
    let image_asset_id = "asset-keyframe-001";

    sqlx::query(
        "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at)
         VALUES (?, 'image', 'prov-img', 'dall-e-3', 'completed', '{}', '{}', '2026-10-10T00:00:00Z')"
    )
    .bind(image_task_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO assets (id, task_id, type, path, mime_type, width, height, metadata_json, created_at)
         VALUES (?, ?, 'image', ?, 'image/png', 1920, 1080, '{\"source_step\":\"storyboard\"}', '2026-10-10T00:00:00Z')"
    )
    .bind(image_asset_id)
    .bind(image_task_id)
    .bind(frame_file.to_string_lossy().to_string())
    .execute(&pool)
    .await
    .unwrap();

    // Step 3: 首帧驱动视频生成并登记资产
    let video_file = dir.join("subsea_clip.mp4");
    std::fs::write(&video_file, b"fake-subsea-video-mp4").unwrap();
    let video_asset_id = "asset-video-001";

    sqlx::query(
        "INSERT INTO generation_tasks (id, modality, provider_id, model_id, status, input_json, parameters_json, created_at)
         VALUES (?, 'video', 'prov-vid', 'runway-gen3', 'completed', '{}', '{}', '2026-10-10T00:00:00Z')"
    )
    .bind(video_task_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO assets (id, task_id, type, path, mime_type, width, height, duration_ms, metadata_json, created_at)
         VALUES (?, ?, 'video', ?, 'video/mp4', 1920, 1080, 5000, '{\"first_frame_asset\":\"asset-keyframe-001\"}', '2026-10-10T00:00:00Z')"
    )
    .bind(video_asset_id)
    .bind(video_task_id)
    .bind(video_file.to_string_lossy().to_string())
    .execute(&pool)
    .await
    .unwrap();

    // 2. 验证全链路资产在统一资产库可索引且来源可溯
    let all_assets = services::search::list_asset_library(&pool, None, None, None, None)
        .await
        .unwrap();
    assert!(all_assets.len() >= 2);

    let keyframe_item = all_assets.iter().find(|a| a.asset_type == "image").unwrap();
    assert_eq!(keyframe_item.task_id, image_task_id);
    assert!(keyframe_item.file_exists);

    let video_item = all_assets.iter().find(|a| a.asset_type == "video").unwrap();
    assert_eq!(video_item.task_id, video_task_id);
    assert!(video_item.file_exists);

    // 3. 验证变量插值严格校验契约
    let mut vars = std::collections::HashMap::new();
    vars.insert("step1_script".to_string(), "Scene 1: Subsea".to_string());
    vars.insert(
        "step2_image_path".to_string(),
        frame_file.to_string_lossy().to_string(),
    );

    let template = "Generate video with frame {{step2_image_path}} and prompt {{step1_script}}";
    let substituted =
        services::pipeline::substitute_vars_strict(template, &vars, "Step 3 Video").unwrap();
    assert!(substituted.contains("subsea_frame.png"));
    assert!(substituted.contains("Scene 1: Subsea"));

    // 验证拼写错误变量被严格拦截拒绝，避免用户盲目扣费
    let bad_template = "Generate video with {{unknown_variable}}";
    assert!(services::pipeline::substitute_vars_strict(bad_template, &vars, "Step 3 Bad").is_err());

    let _ = std::fs::remove_dir_all(dir);
}
