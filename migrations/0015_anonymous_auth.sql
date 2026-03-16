-- Anonymous authentication support
-- Migration: 0005_anonymous_auth.sql

-- Add account_number field for anonymous authentication (Mullvad-style)
ALTER TABLE users ADD COLUMN account_number VARCHAR(16) UNIQUE;

-- Create index for fast lookups by account number
CREATE INDEX IF NOT EXISTS idx_users_account_number ON users(account_number);

-- Ensure only one of github_id, gitlab_id, or account_number can be set per user
ALTER TABLE users ADD CONSTRAINT check_single_auth_method 
CHECK (
    (github_id IS NOT NULL)::int + (gitlab_id IS NOT NULL)::int + (account_number IS NOT NULL)::int <= 1
);
