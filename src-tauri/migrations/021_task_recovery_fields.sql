-- T03: 任务崩溃恢复支持字段
-- 记录远端任务 ID、轮询 URL、执行阶段与计费风险，支持断点续查与防重复计费
ALTER TABLE generation_tasks ADD COLUMN remote_job_id TEXT;
ALTER TABLE generation_tasks ADD COLUMN remote_poll_url TEXT;
ALTER TABLE generation_tasks ADD COLUMN execution_phase TEXT;
ALTER TABLE generation_tasks ADD COLUMN billing_risk TEXT;

CREATE INDEX IF NOT EXISTS idx_generation_tasks_remote_job ON generation_tasks(remote_job_id) WHERE remote_job_id IS NOT NULL;
