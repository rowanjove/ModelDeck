-- 018_fts5.sql: SQLite FTS5 全文搜索与增量同步触发器

-- 1. 创建 FTS5 全文检索虚拟表
CREATE VIRTUAL TABLE IF NOT EXISTS fts_entries USING fts5(
    entity_id UNINDEXED,
    entity_type UNINDEXED,
    title,
    body,
    search_text,
    tokenize = 'unicode61'
);

-- 2. Conversations 增删改同步触发器
CREATE TRIGGER IF NOT EXISTS trg_fts_conversations_ai
AFTER INSERT ON conversations
BEGIN
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'conversation', NEW.title, NEW.title, NEW.title);
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_conversations_au
AFTER UPDATE OF title ON conversations
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'conversation', NEW.title, NEW.title, NEW.title);
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_conversations_ad
AFTER DELETE ON conversations
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
END;

-- 3. Messages 增删改同步触发器
CREATE TRIGGER IF NOT EXISTS trg_fts_messages_ai
AFTER INSERT ON messages
BEGIN
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'message', NEW.role, NEW.content_json, NEW.content_json);
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_messages_au
AFTER UPDATE OF content_json ON messages
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'message', NEW.role, NEW.content_json, NEW.content_json);
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_messages_ad
AFTER DELETE ON messages
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
END;

-- 4. Generation Tasks 增删改同步触发器
CREATE TRIGGER IF NOT EXISTS trg_fts_tasks_ai
AFTER INSERT ON generation_tasks
BEGIN
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'task', NEW.modality, NEW.input_json, NEW.input_json);
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_tasks_au
AFTER UPDATE OF input_json, status, error ON generation_tasks
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'task', NEW.modality, NEW.input_json, NEW.input_json || ' ' || COALESCE(NEW.error, ''));
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_tasks_ad
AFTER DELETE ON generation_tasks
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
END;

-- 5. Agent Memories 增删改同步触发器
CREATE TRIGGER IF NOT EXISTS trg_fts_memories_ai
AFTER INSERT ON agent_memories
BEGIN
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'memory', NEW.key, NEW.content, NEW.key || ' ' || NEW.content);
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_memories_au
AFTER UPDATE OF key, content ON agent_memories
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'memory', NEW.key, NEW.content, NEW.key || ' ' || NEW.content);
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_memories_ad
AFTER DELETE ON agent_memories
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
END;

-- 6. Pipelines 增删改同步触发器
CREATE TRIGGER IF NOT EXISTS trg_fts_pipelines_ai
AFTER INSERT ON pipelines
BEGIN
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'pipeline', NEW.name, NEW.description, NEW.name || ' ' || NEW.description);
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_pipelines_au
AFTER UPDATE OF name, description ON pipelines
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'pipeline', NEW.name, NEW.description, NEW.name || ' ' || NEW.description);
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_pipelines_ad
AFTER DELETE ON pipelines
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
END;

-- 7. Assets 增删改同步触发器
CREATE TRIGGER IF NOT EXISTS trg_fts_assets_ai
AFTER INSERT ON assets
BEGIN
    INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
    VALUES (NEW.id, 'asset', NEW.type, COALESCE(NEW.path, ''), COALESCE(NEW.path, ''));
END;

CREATE TRIGGER IF NOT EXISTS trg_fts_assets_ad
AFTER DELETE ON assets
BEGIN
    DELETE FROM fts_entries WHERE entity_id = OLD.id;
END;

-- 8. 初始化导入现有数据入 FTS 表
INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
SELECT id, 'conversation', title, title, title FROM conversations;

INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
SELECT id, 'message', role, content_json, content_json FROM messages;

INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
SELECT id, 'task', modality, input_json, input_json FROM generation_tasks;

INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
SELECT id, 'memory', key, content, key || ' ' || content FROM agent_memories;

INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
SELECT id, 'pipeline', name, description, name || ' ' || description FROM pipelines;

INSERT INTO fts_entries (entity_id, entity_type, title, body, search_text)
SELECT id, 'asset', type, COALESCE(path, ''), COALESCE(path, '') FROM assets;
