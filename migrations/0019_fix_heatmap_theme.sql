-- no-transaction
-- Fix activity-heatmap plugin: adapt empty cell color for light/dark themes
UPDATE plugin_store
SET
  script = REPLACE(
    script,
    $$if (secs === 0) return 'rgba(255,255,255,0.04)';$$,
    $$if (secs === 0) return isDark ? 'rgba(255,255,255,0.06)' : 'rgba(0,0,0,0.06)';$$
  ),
  version = '1.0.1',
  updated_at = NOW()
WHERE name = 'activity-heatmap'
  AND script LIKE '%rgba(255,255,255,0.04)%'
