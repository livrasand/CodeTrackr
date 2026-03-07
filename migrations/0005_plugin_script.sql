-- no-transaction
-- Add JS script field to plugin_store for client-side plugin execution
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS script TEXT;
