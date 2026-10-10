CREATE TABLE IF NOT EXISTS agent_memories (
  id TEXT PRIMARY KEY,
  conversation_id TEXT,
  scope TEXT NOT NULL,
  key TEXT NOT NULL,
  content TEXT NOT NULL,
  importance INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_memories_conv ON agent_memories(conversation_id);
CREATE INDEX IF NOT EXISTS idx_agent_memories_scope ON agent_memories(scope, key);
