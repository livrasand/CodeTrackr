#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&url).await?;
    
    // Grant badge-generator read access to heartbeats
    sqlx::query(
        "INSERT INTO plugin_permissions (plugin_id, table_name, access_type, requires_user_filter, allowed_columns)
         SELECT id, 'heartbeats', 'read', true, NULL
         FROM plugin_store WHERE name = 'badge-generator'
         ON CONFLICT (plugin_id, table_name) DO NOTHING"
    )
    .execute(&pool)
    .await?;

    println!("Plugin permissions added correctly!");
    Ok(())
}
