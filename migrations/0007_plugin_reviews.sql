-- no-transaction
-- Plugin ratings (1-5 stars, one per user per plugin)
CREATE TABLE IF NOT EXISTS plugin_ratings (
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plugin_id  UUID NOT NULL REFERENCES plugin_store(id) ON DELETE CASCADE,
    rating     SMALLINT NOT NULL CHECK (rating BETWEEN 1 AND 5),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, plugin_id)
);

-- Plugin reviews (optional text, one per user per plugin)
CREATE TABLE IF NOT EXISTS plugin_reviews (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plugin_id  UUID NOT NULL REFERENCES plugin_store(id) ON DELETE CASCADE,
    body       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, plugin_id)
);

-- Plugin screenshots (URL or base64, multiple per plugin per user)
CREATE TABLE IF NOT EXISTS plugin_screenshots (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_id  UUID NOT NULL REFERENCES plugin_store(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    url        TEXT NOT NULL,
    caption    TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Cached avg rating on plugin_store
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS avg_rating NUMERIC(3,2) NOT NULL DEFAULT 0;
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS rating_count INTEGER NOT NULL DEFAULT 0;

-- Trigger to keep avg_rating and rating_count updated
CREATE OR REPLACE FUNCTION update_plugin_rating() RETURNS TRIGGER AS $$
BEGIN
    UPDATE plugin_store SET
        avg_rating   = (SELECT COALESCE(AVG(rating), 0) FROM plugin_ratings WHERE plugin_id = COALESCE(NEW.plugin_id, OLD.plugin_id)),
        rating_count = (SELECT COUNT(*) FROM plugin_ratings WHERE plugin_id = COALESCE(NEW.plugin_id, OLD.plugin_id))
    WHERE id = COALESCE(NEW.plugin_id, OLD.plugin_id);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_plugin_rating ON plugin_ratings;
CREATE TRIGGER trg_plugin_rating
AFTER INSERT OR UPDATE OR DELETE ON plugin_ratings
FOR EACH ROW EXECUTE FUNCTION update_plugin_rating();
