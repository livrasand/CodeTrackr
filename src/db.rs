use sqlx::{PgPool, postgres::PgPoolOptions};
use anyhow::Result;

/// Split SQL text into individual statements on `;`, but do NOT split
/// inside dollar-quoted blocks (`$$...$$` or `$tag$...$tag$`).
fn split_sql_statements(sql: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut start = 0;
    let mut i = 0;

    while i < len {
        if bytes[i] == b'$' {
            // Look for a dollar-quote tag: $[optional_tag]$
            let mut j = i + 1;
            while j < len && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if j < len && bytes[j] == b'$' {
                // Found opening tag, e.g. `$$` or `$tag$`
                let tag = &sql[i..=j];
                let tag_end = j + 1;
                // Search for the matching closing tag
                if let Some(pos) = sql[tag_end..].find(tag) {
                    i = tag_end + pos + tag.len();
                    continue;
                } else {
                    // No closing tag found; treat rest as one block
                    break;
                }
            }
        }

        if bytes[i] == b';' {
            statements.push(sql[start..i].trim());
            start = i + 1;
        }

        i += 1;
    }

    // Trailing content after last `;`
    let tail = sql[start..].trim();
    if !tail.is_empty() {
        statements.push(tail);
    }

    statements
}

#[derive(Clone)]
pub struct Database {
    pub pool: PgPool,
}

impl Database {
    pub async fn new(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .min_connections(2)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(url)
            .await?;

        Ok(Self { pool })
    }

    pub async fn migrate_with_url(&self, _url: &str) -> Result<()> {
        tracing::info!("Running database migrations...");

        // Diagnose: log current statement_timeout from server
        let timeout: (String,) = sqlx::query_as("SELECT current_setting('statement_timeout')")
            .fetch_one(&self.pool)
            .await
            .unwrap_or_else(|_| (String::from("unknown"),));
        tracing::info!("Server statement_timeout = {:?}", timeout.0);

        // Ensure the migrations tracking table exists before querying it
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS _sqlx_migrations (
                version        BIGINT PRIMARY KEY,
                description    TEXT NOT NULL,
                installed_on   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                success        BOOLEAN NOT NULL DEFAULT false,
                checksum       BYTEA NOT NULL,
                execution_time BIGINT NOT NULL DEFAULT 0
            )"#
        )
        .execute(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create _sqlx_migrations table: {}", e))?;

        let mut migrator_base = sqlx::migrate!("./migrations");
        let migrator = migrator_base.set_ignore_missing(true);

        // Check pending migrations without acquiring a lock
        let applied: Vec<(i64,)> = sqlx::query_as(
            "SELECT version FROM _sqlx_migrations WHERE success = true"
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let applied_versions: std::collections::HashSet<i64> =
            applied.into_iter().map(|(v,)| v).collect();

        let has_pending = migrator
            .migrations
            .iter()
            .any(|m| !applied_versions.contains(&m.version));

        if !has_pending {
            tracing::info!("No pending migrations, skipping.");
            return Ok(());
        }

        tracing::info!("Pending migrations found, applying manually (bypass advisory lock)...");
        for migration in migrator.migrations.iter() {
            if applied_versions.contains(&migration.version) {
                continue;
            }
            tracing::info!("Applying migration {}: {}...", migration.version, migration.description);
            let sql: &str = migration.sql.as_ref();
            // Execute each statement individually to stay under the statement_timeout.
            // Split on ';' but ignore semicolons inside dollar-quoted blocks ($$...$$).
            for raw_stmt in split_sql_statements(sql) {
                // Strip comment-only lines, keep SQL lines
                let stmt: String = raw_stmt
                    .lines()
                    .filter(|line| {
                        let t = line.trim();
                        !t.is_empty() && !t.starts_with("--")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let stmt = stmt.trim();
                if stmt.is_empty() {
                    continue;
                }
                sqlx::query(stmt)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("Migration {} failed on statement [{}]: {}", migration.version, stmt, e))?;
            }
            // Register migration in _sqlx_migrations
            let checksum = Vec::from(migration.checksum.as_ref());
            sqlx::query(
                r#"INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
                   VALUES ($1, $2, NOW(), true, $3, 0)
                   ON CONFLICT (version) DO UPDATE SET success = true, checksum = $3"#
            )
            .bind(migration.version)
            .bind(migration.description.as_ref())
            .bind(&checksum)
            .execute(&self.pool)
            .await?;
            tracing::info!("Migration {} applied.", migration.version);
        }
        tracing::info!("Migrations complete.");
        Ok(())
    }
}
