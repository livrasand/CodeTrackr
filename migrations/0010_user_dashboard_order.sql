-- 0010_user_dashboard_order.sql
-- Add table to store user preferences for dashboard panel order

CREATE TABLE IF NOT EXISTS user_dashboard_order (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    panel_name TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (user_id, panel_name)
);
