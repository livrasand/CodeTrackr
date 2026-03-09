-- no-transaction
-- Theme store: community-published CSS variable themes

CREATE TABLE IF NOT EXISTS theme_store (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    author_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name          VARCHAR(64) NOT NULL UNIQUE,
    display_name  VARCHAR(128) NOT NULL,
    description   TEXT,
    version       VARCHAR(16) NOT NULL DEFAULT '1.0.0',
    icon          VARCHAR(8) NOT NULL DEFAULT '🎨',
    -- CSS variables as JSON: { "--bg": "#fff", "--text-main": "#000", ... }
    variables     JSONB NOT NULL DEFAULT '{}',
    -- Optional raw CSS for advanced overrides (sanitized on insert)
    custom_css    TEXT,
    is_published  BOOLEAN NOT NULL DEFAULT true,
    is_banned     BOOLEAN NOT NULL DEFAULT false,
    install_count INTEGER NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_theme_store_author ON theme_store(author_id);
CREATE INDEX IF NOT EXISTS idx_theme_store_published ON theme_store(is_published, is_banned);

CREATE TABLE IF NOT EXISTS installed_themes (
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    theme_id      UUID NOT NULL REFERENCES theme_store(id) ON DELETE CASCADE,
    installed_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, theme_id)
);

-- Active theme preference + custom CSS overrides per user
CREATE TABLE IF NOT EXISTS user_theme_prefs (
    user_id       UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    active_theme_id UUID REFERENCES theme_store(id) ON DELETE SET NULL,
    -- User's own CSS variable overrides (applied on top of active theme)
    custom_vars   JSONB NOT NULL DEFAULT '{}',
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
