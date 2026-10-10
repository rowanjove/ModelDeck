-- 017_foreign_keys_and_cascade.sql: 级联删除保障与孤儿数据防护
-- 1. 当删除会话时，级联清理其会话级长期记忆
CREATE TRIGGER IF NOT EXISTS trg_cascade_delete_conversation_memories
AFTER DELETE ON conversations
BEGIN
    DELETE FROM agent_memories WHERE conversation_id = OLD.id;
END;

-- 2. 当删除历史生成任务时，级联清理其所有登记的媒体资产记录
CREATE TRIGGER IF NOT EXISTS trg_cascade_delete_task_assets
AFTER DELETE ON generation_tasks
BEGIN
    DELETE FROM assets WHERE task_id = OLD.id;
END;
