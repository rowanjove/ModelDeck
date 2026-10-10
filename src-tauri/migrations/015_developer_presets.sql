CREATE TABLE IF NOT EXISTS api_test_presets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    provider_id TEXT,
    headers_json TEXT NOT NULL,
    body_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_api_test_presets_updated ON api_test_presets(updated_at DESC);
