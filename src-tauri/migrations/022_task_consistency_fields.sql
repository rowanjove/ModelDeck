-- T08: 任务错误码与重试幂等标记
ALTER TABLE generation_tasks ADD COLUMN error_code TEXT;
ALTER TABLE generation_tasks ADD COLUMN retryable INTEGER;
