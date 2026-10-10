CREATE TABLE IF NOT EXISTS generation_tasks (
  id TEXT PRIMARY KEY,
  modality TEXT NOT NULL,
  provider_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  status TEXT NOT NULL,
  input_json TEXT NOT NULL,
  parameters_json TEXT NOT NULL,
  raw_request_json TEXT,
  raw_response_json TEXT,
  error TEXT,
  created_at TEXT NOT NULL,
  completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_generation_tasks_created ON generation_tasks(created_at, id);

CREATE TABLE IF NOT EXISTS assets (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  type TEXT NOT NULL,
  path TEXT,
  url TEXT,
  mime_type TEXT,
  width INTEGER,
  height INTEGER,
  duration_ms INTEGER,
  metadata_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_assets_task ON assets(task_id);
