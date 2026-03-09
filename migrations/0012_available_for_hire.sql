-- no-transaction
-- Available for hire field on users
ALTER TABLE users ADD COLUMN IF NOT EXISTS available_for_hire BOOLEAN NOT NULL DEFAULT false;
