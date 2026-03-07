-- no-transaction
-- Store the last version accepted by the user for each installed plugin
ALTER TABLE installed_plugins ADD COLUMN IF NOT EXISTS accepted_version VARCHAR(16);
ALTER TABLE installed_plugins ADD COLUMN IF NOT EXISTS accepted_script TEXT;
