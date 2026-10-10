-- 016_hardening_indexes.sql: 性能加固与状态索引

-- 1. 任务状态与调度查询索引（用于 Crash Recovery 与未完成任务轮询）
CREATE INDEX IF NOT EXISTS idx_generation_tasks_status_created ON generation_tasks(status, created_at DESC);

-- 2. 任务模态查询索引（用于 History 按模态过滤）
CREATE INDEX IF NOT EXISTS idx_generation_tasks_modality_created ON generation_tasks(modality, created_at DESC);

-- 3. 对话更新时间索引（用于会话列表高性能排序）
CREATE INDEX IF NOT EXISTS idx_conversations_updated ON conversations(updated_at DESC);

-- 4. 资产库时间索引（用于资产库高性能分页）
CREATE INDEX IF NOT EXISTS idx_assets_created ON assets(created_at DESC);
