ALTER TABLE generation_tasks ADD COLUMN queue_output_json TEXT;

-- A tombstone is permanent. Prevent late INSERT/UPSERT and UPDATE paths from
-- recreating a task after its history was deleted.
CREATE TRIGGER IF NOT EXISTS trg_no_resurrect_deleted_task_insert
BEFORE INSERT ON generation_tasks
WHEN EXISTS (
  SELECT 1 FROM deleted_generation_tasks WHERE id = NEW.id
)
BEGIN
  SELECT RAISE(ABORT, 'generation task id was deleted');
END;

CREATE TRIGGER IF NOT EXISTS trg_no_resurrect_deleted_task_update
BEFORE UPDATE ON generation_tasks
WHEN EXISTS (
  SELECT 1 FROM deleted_generation_tasks WHERE id = NEW.id
)
BEGIN
  SELECT RAISE(ABORT, 'generation task id was deleted');
END;

-- Developer presets survive provider removal as standalone requests, while
-- clearing the missing association so reopening a preset cannot target a
-- deleted provider.
UPDATE api_test_presets
   SET provider_id = NULL
 WHERE provider_id IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM providers WHERE providers.id = api_test_presets.provider_id);

CREATE TRIGGER IF NOT EXISTS trg_clear_deleted_provider_from_api_presets
AFTER DELETE ON providers
BEGIN
  UPDATE api_test_presets SET provider_id = NULL WHERE provider_id = OLD.id;
END;
