-- no-transaction
-- Plugin dependencies: allow plugins to reference others with @ in description
CREATE TABLE IF NOT EXISTS plugin_dependencies (
    plugin_id       UUID NOT NULL REFERENCES plugin_store(id) ON DELETE CASCADE,
    dependency_name VARCHAR(64) NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (plugin_id, dependency_name)
);

-- Index for faster lookups
CREATE INDEX IF NOT EXISTS idx_plugin_dependencies_plugin_id ON plugin_dependencies(plugin_id);
