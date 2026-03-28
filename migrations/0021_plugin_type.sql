-- no-transaction
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS plugin_type VARCHAR(32) NOT NULL DEFAULT 'widget';
