CREATE TABLE IF NOT EXISTS model_pricing (
  model_id TEXT PRIMARY KEY,
  input_per_m REAL NOT NULL DEFAULT 0.0,
  output_per_m REAL NOT NULL DEFAULT 0.0,
  currency TEXT NOT NULL DEFAULT 'USD',
  updated_at TEXT NOT NULL
);
