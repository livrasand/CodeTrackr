-- no-transaction
-- CodeTrackr Initial Schema
-- Migration: 0001_initial.sql

-- ── Users ─────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username      VARCHAR(64) NOT NULL UNIQUE,
    display_name  VARCHAR(128),
    email         VARCHAR(256),
    avatar_url    TEXT,
    github_id     VARCHAR(64) UNIQUE,
    gitlab_id     VARCHAR(64) UNIQUE,
    plan          VARCHAR(16) NOT NULL DEFAULT 'free',  -- 'free' | 'pro'
    is_public     BOOLEAN NOT NULL DEFAULT true,
    country       VARCHAR(64),
    timezone      VARCHAR(64) NOT NULL DEFAULT 'UTC',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_users_github_id ON users(github_id);
CREATE INDEX IF NOT EXISTS idx_users_gitlab_id ON users(gitlab_id);
CREATE INDEX IF NOT EXISTS idx_users_username  ON users(username);

-- ── API Keys ──────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS api_keys (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         VARCHAR(128) NOT NULL DEFAULT 'Default Key',
    key_hash     VARCHAR(64) NOT NULL UNIQUE,   -- SHA-256 of the actual key
    key_prefix   VARCHAR(16) NOT NULL,           -- First 12 chars for display
    last_used_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_api_keys_user_id  ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys(key_hash);

-- ── Heartbeats ────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS heartbeats (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project          VARCHAR(256) NOT NULL,
    file             TEXT,
    language         VARCHAR(64),
    branch           VARCHAR(128),
    commit           VARCHAR(64),
    workspace_root   TEXT,
    package_path     TEXT,       -- monorepo sub-package
    duration_seconds INTEGER NOT NULL DEFAULT 30,
    is_write         BOOLEAN NOT NULL DEFAULT false,
    editor           VARCHAR(64),
    os               VARCHAR(64),
    machine          VARCHAR(128),
    recorded_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_heartbeats_user_id     ON heartbeats(user_id);
CREATE INDEX IF NOT EXISTS idx_heartbeats_recorded_at ON heartbeats(recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_heartbeats_user_time   ON heartbeats(user_id, recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_heartbeats_language    ON heartbeats(language);
CREATE INDEX IF NOT EXISTS idx_heartbeats_project     ON heartbeats(project);

-- ── Plugin Settings (per user, per plugin) ────────────────────────────────────
CREATE TABLE IF NOT EXISTS plugin_settings (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plugin     VARCHAR(64) NOT NULL,
    settings   JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, plugin)
);

CREATE INDEX IF NOT EXISTS idx_plugin_settings_user_plugin ON plugin_settings(user_id, plugin);

-- ── Daily stats cache (materialized periodically) ─────────────────────────────
CREATE TABLE IF NOT EXISTS daily_stats_cache (
    user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    date             DATE NOT NULL,
    total_seconds    BIGINT NOT NULL DEFAULT 0,
    top_language     VARCHAR(64),
    top_project      VARCHAR(256),
    computed_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, date)
);
