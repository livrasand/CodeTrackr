-- Add is_anonymous flag for better privacy handling
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_anonymous BOOLEAN NOT NULL DEFAULT FALSE;

-- Update existing anonymous users to set the flag
UPDATE users SET is_anonymous = TRUE WHERE account_number IS NOT NULL;

-- Create index for performance
CREATE INDEX IF NOT EXISTS idx_users_is_anonymous ON users(is_anonymous);

-- Reinforce the single auth method constraint with explicit triggers
-- This ensures data integrity even if the constraint is bypassed
CREATE OR REPLACE FUNCTION enforce_single_auth_method()
RETURNS TRIGGER AS $$
DECLARE
    auth_count integer;
BEGIN
    -- Count how many auth methods are set
    auth_count := (
        (NEW.github_id IS NOT NULL)::int +
        (NEW.gitlab_id IS NOT NULL)::int +
        (NEW.account_number IS NOT NULL)::int
    );
    
    IF auth_count > 1 THEN
        RAISE EXCEPTION 'User can have only one authentication method (github_id, gitlab_id, or account_number)';
    END IF;
    
    -- Ensure is_anonymous flag matches account_number presence
    IF NEW.account_number IS NOT NULL AND NEW.is_anonymous = FALSE THEN
        NEW.is_anonymous := TRUE;
    ELSIF NEW.account_number IS NULL AND NEW.is_anonymous = TRUE THEN
        NEW.is_anonymous := FALSE;
    END IF;
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger to enforce the constraint
DROP TRIGGER IF EXISTS check_single_auth_trigger ON users;
CREATE TRIGGER check_single_auth_trigger
    BEFORE INSERT OR UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION enforce_single_auth_method();
