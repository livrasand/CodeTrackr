-- no-transaction
CREATE TABLE IF NOT EXISTS plugin_store (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name          VARCHAR(64) NOT NULL UNIQUE,
    display_name  VARCHAR(128) NOT NULL,
    description   TEXT,
    version       VARCHAR(16) NOT NULL DEFAULT '0.1.0',
    repository    TEXT,
    is_published  BOOLEAN NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS installed_plugins (
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plugin_id     UUID NOT NULL REFERENCES plugin_store(id) ON DELETE CASCADE,
    installed_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, plugin_id)
);
