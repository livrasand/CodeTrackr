-- Mark all current plugins as published so they show up in the store
UPDATE plugin_store SET is_published = true WHERE is_published = false;
