-- no-transaction
-- Add is_admin flag to users
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_admin BOOLEAN NOT NULL DEFAULT false;

-- Add rich metadata + moderation fields to plugin_store
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS icon VARCHAR(8) DEFAULT '🔌';
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS widget_type VARCHAR(32) DEFAULT 'counter';
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS api_endpoint TEXT;
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS settings_schema JSONB;
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS is_banned BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS ban_reason TEXT;
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS install_count INTEGER NOT NULL DEFAULT 0;

-- Plugin reports by users
CREATE TABLE IF NOT EXISTS plugin_reports (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_id     UUID NOT NULL REFERENCES plugin_store(id) ON DELETE CASCADE,
    reporter_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason        VARCHAR(32) NOT NULL CHECK (reason IN ('spam','malware','inappropriate','broken','other')),
    description   TEXT,
    resolved      BOOLEAN NOT NULL DEFAULT false,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (plugin_id, reporter_id)
);

-- Update install_count on install/uninstall via trigger
CREATE OR REPLACE FUNCTION update_install_count() RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE plugin_store SET install_count = install_count + 1 WHERE id = NEW.plugin_id;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE plugin_store SET install_count = GREATEST(install_count - 1, 0) WHERE id = OLD.plugin_id;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_install_count ON installed_plugins;
CREATE TRIGGER trg_install_count
AFTER INSERT OR DELETE ON installed_plugins
FOR EACH ROW EXECUTE FUNCTION update_install_count();
