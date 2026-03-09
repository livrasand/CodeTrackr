use axum::response::{Html, IntoResponse};

/// Serve the main index.html for all unmatched routes (SPA pattern)
pub async fn serve_index() -> impl IntoResponse {
    match tokio::fs::read_to_string("static/index.html").await {
        Ok(content) => Html(content).into_response(),
        Err(_) => Html(FALLBACK_HTML.to_string()).into_response()
    }
}

pub async fn serve_docs() -> impl IntoResponse {
    match tokio::fs::read_to_string("static/docs.html").await {
        Ok(content) => Html(content).into_response(),
        Err(_) => serve_index().await.into_response(),
    }
}

pub async fn serve_plugin_store() -> impl IntoResponse {
    match tokio::fs::read_to_string("static/plugin_store.html").await {
        Ok(content) => Html(content).into_response(),
        Err(_) => serve_index().await.into_response(),
    }
}

const FALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8"/>
  <title>CodeTrackr — Loading...</title>
  <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
  <style>
    body { background: #0d0d0d; color: #fff; font-family: sans-serif;
           display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
    .logo { font-size: 2rem; font-weight: bold; }
    .sub { color: #888; margin-top: 8px; font-size: 0.9rem; }
  </style>
</head>
<body>
  <div style="text-align:center">
    <div class="logo">⚡ CodeTrackr</div>
    <div class="sub">Server running — frontend static files not found.</div>
    <div class="sub">Run <code>npm run build</code> in the frontend directory.</div>
    <br/>
    <div class="sub"><a style="color:#7c3aed" href="/api/v1/health">API Health Check →</a></div>
  </div>
</body>
</html>"#;
