-- Keep a durable tombstone for tasks deleted while a worker may still finish.
-- Late result upserts must not recreate a task the user explicitly deleted.
CREATE TABLE IF NOT EXISTS deleted_generation_tasks (
  id TEXT PRIMARY KEY,
  deleted_at TEXT NOT NULL
);

-- Presets are dependent on models, but the legacy table did not declare a
-- foreign key. Keep future model deletion from leaving unusable presets.
CREATE TRIGGER IF NOT EXISTS trg_cascade_delete_model_presets
AFTER DELETE ON models
BEGIN
  DELETE FROM presets WHERE model_id = OLD.id;
END;

-- Queue submissions historically wrote batch/pipeline input objects without
-- the common `kind` discriminator. Normalize those writes at the storage
-- boundary so old and new producers remain readable by history.
CREATE TRIGGER IF NOT EXISTS trg_generation_tasks_default_kind_insert
AFTER INSERT ON generation_tasks
WHEN json_valid(NEW.input_json)
  AND json_type(NEW.input_json) = 'object'
  AND (json_extract(NEW.input_json, '$.kind') IS NULL
       OR trim(CAST(json_extract(NEW.input_json, '$.kind') AS TEXT)) = '')
BEGIN
  UPDATE generation_tasks
     SET input_json = json_set(NEW.input_json, '$.kind', NEW.modality)
   WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_generation_tasks_default_kind_update
AFTER UPDATE OF input_json ON generation_tasks
WHEN json_valid(NEW.input_json)
  AND json_type(NEW.input_json) = 'object'
  AND (json_extract(NEW.input_json, '$.kind') IS NULL
       OR trim(CAST(json_extract(NEW.input_json, '$.kind') AS TEXT)) = '')
BEGIN
  UPDATE generation_tasks
     SET input_json = json_set(NEW.input_json, '$.kind', NEW.modality)
   WHERE id = NEW.id;
END;
