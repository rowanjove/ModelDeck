CREATE TABLE IF NOT EXISTS conversations (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  mode TEXT NOT NULL,
  model_id TEXT NOT NULL,
  system_prompt TEXT NOT NULL,
  context_strategy TEXT NOT NULL,
  context_limit INTEGER NOT NULL,
  summary TEXT,
  summary_until TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  content_json TEXT NOT NULL,
  token_count INTEGER NOT NULL,
  raw_request_json TEXT,
  raw_response_json TEXT,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id, created_at);
