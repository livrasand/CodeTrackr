-- no-transaction
-- Contact requests for developers available for hire
CREATE TABLE IF NOT EXISTS hire_contacts (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_user_id  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sender_name     TEXT NOT NULL,
    sender_email    TEXT NOT NULL,
    message         TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_hire_contacts_target ON hire_contacts(target_user_id);
