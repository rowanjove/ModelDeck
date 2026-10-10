-- T14: Multimodal pricing, pricing unit, source, and provider affinity
ALTER TABLE model_pricing ADD COLUMN provider_id TEXT NOT NULL DEFAULT '';
ALTER TABLE model_pricing ADD COLUMN pricing_unit TEXT NOT NULL DEFAULT 'per_1m_tokens';
ALTER TABLE model_pricing ADD COLUMN price_per_unit REAL NOT NULL DEFAULT 0.0;
ALTER TABLE model_pricing ADD COLUMN source TEXT NOT NULL DEFAULT 'custom_user';
