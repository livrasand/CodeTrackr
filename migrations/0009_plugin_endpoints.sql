-- no-transaction
-- Plugin RPC endpoints: stores declared HTTP handlers per plugin script
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS endpoints JSONB;
