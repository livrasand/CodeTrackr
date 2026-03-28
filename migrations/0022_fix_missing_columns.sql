-- no-transaction
-- Fix: ensure columns added in previous migrations actually exist in the DB
ALTER TABLE users ADD COLUMN IF NOT EXISTS show_in_leaderboard BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE plugin_store ADD COLUMN IF NOT EXISTS plugin_type VARCHAR(32) NOT NULL DEFAULT 'widget';
