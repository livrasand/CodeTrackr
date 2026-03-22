-- Add anonymous account support
ALTER TABLE users ADD COLUMN IF NOT EXISTS account_number VARCHAR(16) UNIQUE;
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_anonymous BOOLEAN NOT NULL DEFAULT false;

-- Create index for account number lookups
CREATE INDEX IF NOT EXISTS idx_users_account_number ON users(account_number);
