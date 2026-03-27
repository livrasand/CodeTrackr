-- no-transaction
-- Add field to track if plugin accesses external network services
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS has_external_access BOOLEAN NOT NULL DEFAULT false;
