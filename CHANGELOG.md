# CHANGELOG

All notable schema changes to CodeTrackr are documented here.  
Current schema version: **0014**

---

## [0014] — `0014_themes.sql`

- **`theme_store`** — community-published CSS theme store. Stores CSS variables as JSON (`variables JSONB`) and optional custom CSS (`custom_css TEXT`).
- **`installed_themes`** — user ↔ installed theme relationship.
- **`user_theme_prefs`** — active theme preference per user, with custom CSS variable overrides (`custom_vars JSONB`).
- Indexes: `idx_theme_store_author`, `idx_theme_store_published`.

---

## [0013] — `0013_hire_contacts.sql`

- **`hire_contacts`** — contact requests to developers available for hire. Fields: `target_user_id`, `sender_name`, `sender_email`, `message`.
- Index: `idx_hire_contacts_target`.

---

## [0012] — `0012_available_for_hire.sql`

- Column `available_for_hire BOOLEAN DEFAULT false` in `users`.

---

## [0011] — `0011_plugin_dependencies.sql`

- **`plugin_dependencies`** — dependencies between plugins (allows referencing other plugins with `@` in description). Composite primary key `(plugin_id, dependency_name)`.
- Index: `idx_plugin_dependencies_plugin_id`.

---

## [0010] — `0010_user_dashboard_order.sql`

- **`user_dashboard_order`** — dashboard panel order per user. Composite primary key `(user_id, panel_name)` with `position INTEGER` field.

---

## [0009] — `0009_plugin_endpoints.sql`

- Column `endpoints JSONB` in `plugin_store` — stores HTTP handlers declared by the plugin script (RPC).

---

## [0008] — `0008_profiles.sql`

- Public profile customization columns in `users`: `bio`, `website`, `profile_show_languages`, `profile_show_projects`, `profile_show_activity`, `profile_show_plugins`, `profile_show_streak`.
- **`user_follows`** — user follow relationship (`follower_id` → `following_id`). Includes `follower_id <> following_id` constraint.
- Indexes: `idx_follows_follower`, `idx_follows_following`.

---

## [0007] — `0007_plugin_reviews.sql`

- **`plugin_ratings`** — 1–5 star rating per user per plugin.
- **`plugin_reviews`** — text review per user per plugin.
- **`plugin_screenshots`** — screenshots (URL or base64) associated with a plugin.
- Columns `avg_rating NUMERIC(3,2)` and `rating_count INTEGER` in `plugin_store` (cache).
- Trigger `trg_plugin_rating` + function `update_plugin_rating()` — keeps `avg_rating` and `rating_count` automatically synchronized.

---

## [0006] — `0006_plugin_accepted_version.sql`

- Columns `accepted_version VARCHAR(16)` and `accepted_script TEXT` in `installed_plugins` — store the latest plugin version and script accepted by the user.

---

## [0005] — `0005_plugin_script.sql`

- Column `script TEXT` in `plugin_store` — stores the JavaScript script for plugin client-side execution.

---

## [0004] — `0004_stripe.sql`

- Billing columns in `users`: `stripe_customer_id VARCHAR(64)`, `stripe_subscription_id VARCHAR(64)`, `plan_expires_at TIMESTAMPTZ`.
- Index: `idx_users_stripe_customer`.

---

## [0003] — `0003_plugin_store_v2.sql`

- Column `is_admin BOOLEAN DEFAULT false` in `users`.
- Columns in `plugin_store`: `icon VARCHAR(8)`, `widget_type VARCHAR(32)`, `api_endpoint TEXT`, `settings_schema JSONB`, `is_banned BOOLEAN`, `ban_reason TEXT`, `install_count INTEGER`.
- **`plugin_reports`** — user plugin reports. Allowed reasons: `spam`, `malware`, `inappropriate`, `broken`, `other`. One report per user per plugin.
- Trigger `trg_install_count` + function `update_install_count()` — automatically increments/decrements `install_count` on install/uninstall.

---

## [0002] — `0002_plugin_store.sql`

- **`plugin_store`** — published plugins catalog. Fields: `author_id`, `name` (unique), `display_name`, `description`, `version`, `repository`, `is_published`.
- **`installed_plugins`** — user ↔ installed plugin relationship. Composite primary key `(user_id, plugin_id)`.

---

## [0001] — `0001_initial.sql` — Initial Schema

- **`users`** — main users table. Supports GitHub and GitLab login (`github_id`, `gitlab_id`). Plan `free` | `pro`.
- **`api_keys`** — API keys per user (SHA-256 hash + display prefix).
- **`heartbeats`** — coding activity records: project, file, language, branch, commit, editor, OS, machine.
- **`plugin_settings`** — plugin configuration per user (JSONB), unique key `(user_id, plugin)`.
- **`daily_stats_cache`** — materialized cache of daily statistics per user.
- Indexes in `users`: `github_id`, `gitlab_id`, `username`.
- Indexes in `heartbeats`: `user_id`, `recorded_at DESC`, `(user_id, recorded_at DESC)`, `language`, `project`.
