CREATE TABLE IF NOT EXISTS tool_permissions (
  tool_name TEXT PRIMARY KEY,
  level TEXT NOT NULL
);

INSERT OR IGNORE INTO tool_permissions (tool_name, level) VALUES
  ('current_time', 'safe'),
  ('read_file', 'safe'),
  ('list_directory', 'safe'),
  ('memory', 'safe'),
  ('http_request', 'confirm'),
  ('write_file', 'confirm'),
  ('delete_file', 'confirm'),
  ('run_command', 'confirm');
