-- Anonymous authentication support
-- Migration: 0005_anonymous_auth.sql

-- Add account_number field for anonymous authentication (Mullvad-style)
ALTER TABLE users ADD COLUMN IF NOT EXISTS account_number VARCHAR(16) UNIQUE;

-- Ensure fast lookups by account number
CREATE INDEX IF NOT EXISTS idx_users_account_number ON users(account_number);

-- Ensure only one auth method is set
ALTER TABLE users DROP CONSTRAINT IF EXISTS check_single_auth_method;
ALTER TABLE users ADD CONSTRAINT check_single_auth_method 
CHECK (
    (github_id IS NOT NULL)::int + (gitlab_id IS NOT NULL)::int + (account_number IS NOT NULL)::int <= 1
);
