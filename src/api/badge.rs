//! Public SVG badge endpoint — no authentication required.
//!
//! Route: `GET /badge/:username/:metric`
//!
//! The server resolves the username to a user_id, enforces `is_public`, then
//! invokes the `badge-generator` plugin's handler named after `:metric` using
//! the target user's context. The plugin is responsible for all badge logic;
//! if it returns a JSON object with `label`/`message` fields the server wraps
//! it into a Shields.io-style flat SVG. If the plugin returns a raw SVG string
//! the server forwards it as-is.
//!
//! Security notes:
//! - Users that are private or non-existent receive the **same** response to
//!   prevent username enumeration.
//! - All user-supplied strings inserted into the SVG are XML-escaped.
//! - Cache-Control headers allow CDN/browser caching for 1 hour.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    AppState,
    api::plugin_rpc::sandbox::run_rpc_in_quickjs,
};

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BadgeQuery {
    /// Badge style: flat (default), flat-square, plastic
    pub style: Option<String>,
}

// ── SVG helpers ───────────────────────────────────────────────────────────────

/// Escapes characters that are special in XML/SVG so user data cannot break out.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Renders a minimal Shields.io-style flat badge SVG.
///
/// `label` goes in the dark left section, `message` in the colored right section.
/// All values are XML-escaped before insertion.
fn render_flat_svg(label: &str, message: &str, color: &str) -> String {
    let label   = xml_escape(label);
    let message = xml_escape(message);
    let color   = xml_escape(color);

    // ~6.5 px per char, 10 px padding each side
    let label_w:  u32 = (label.len()   as f32 * 6.5 + 20.0).round() as u32;
    let msg_w:    u32 = (message.len() as f32 * 6.5 + 20.0).round() as u32;
    let total_w:  u32 = label_w + msg_w;

    // Text x-midpoints in normal px (not the ×10 hack)
    let label_x: f32 = label_w as f32 / 2.0;
    let msg_x:   f32 = label_w as f32 + msg_w as f32 / 2.0;

    let mut s = String::with_capacity(900);
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{tw}\" height=\"20\" role=\"img\" aria-label=\"{l}: {m}\">",
        tw = total_w, l = label, m = message
    ));
    s.push_str(&format!("<title>{}: {}</title>", label, message));

    // Gradient overlay
    s.push_str("<linearGradient id=\"s\" x2=\"0\" y2=\"100%\">");
    s.push_str("<stop offset=\"0\" stop-color=\"#bbb\" stop-opacity=\".1\"/>");
    s.push_str("<stop offset=\"1\" stop-opacity=\".1\"/>");
    s.push_str("</linearGradient>");

    // Rounded clip
    s.push_str("<clipPath id=\"r\">");
    s.push_str(&format!("<rect width=\"{tw}\" height=\"20\" rx=\"3\" fill=\"#fff\"/>", tw = total_w));
    s.push_str("</clipPath>");

    // Label + message rectangles
    s.push_str("<g clip-path=\"url(#r)\">");
    s.push_str(&format!("<rect width=\"{lw}\" height=\"20\" fill=\"#555\"/>", lw = label_w));
    s.push_str(&format!(
        "<rect x=\"{lw}\" width=\"{mw}\" height=\"20\" fill=\"{c}\"/>",
        lw = label_w, mw = msg_w, c = color
    ));
    s.push_str(&format!("<rect width=\"{tw}\" height=\"20\" fill=\"url(#s)\"/>", tw = total_w));
    s.push_str("</g>");

    // Text — shadow then actual
    s.push_str("<g fill=\"#fff\" font-family=\"DejaVu Sans,Verdana,Geneva,sans-serif\" font-size=\"11\" text-anchor=\"middle\">");

    // Label shadow
    s.push_str(&format!(
        "<text x=\"{x:.1}\" y=\"15\" fill=\"#010101\" fill-opacity=\".3\">{l}</text>",
        x = label_x + 0.5, l = label
    ));
    // Label fill
    s.push_str(&format!(
        "<text x=\"{x:.1}\" y=\"14\">{l}</text>",
        x = label_x, l = label
    ));

    // Message shadow
    s.push_str(&format!(
        "<text x=\"{x:.1}\" y=\"15\" fill=\"#010101\" fill-opacity=\".3\">{m}</text>",
        x = msg_x + 0.5, m = message
    ));
    // Message fill
    s.push_str(&format!(
        "<text x=\"{x:.1}\" y=\"14\">{m}</text>",
        x = msg_x, m = message
    ));

    s.push_str("</g></svg>");
    s
}


/// Returns the same "private profile" badge for both non-existent and private
/// users — hides the difference to prevent username enumeration.
fn private_badge_svg() -> String {
    render_flat_svg("codetrackr", "profile private", "#9f9f9f")
}

/// Returns an error badge (plugin not installed, handler not found, etc.)
fn error_badge_svg(msg: &str) -> String {
    render_flat_svg("codetrackr", msg, "#e05d44")
}

// ── SVG response builder ──────────────────────────────────────────────────────

fn svg_response(svg: String, status: StatusCode) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "image/svg+xml".parse().unwrap());
    // Allow GitHub's CDN and the user's browser to cache for 1 hour.
    headers.insert("Cache-Control", "max-age=3600, s-maxage=3600".parse().unwrap());
    // Prevent MIME sniffing — the response is always SVG, never executable.
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    // Required for GitHub's image proxy and any cross-origin embed.
    headers.insert("Access-Control-Allow-Origin", "*".parse().unwrap());

    (status, headers, svg).into_response()
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /badge/:username/:metric`
///
/// Public endpoint — no `Authorization` header required.
///
/// 1. Resolves `:username` → `user_id`  
/// 2. Enforces `is_public = true`  
/// 3. Invokes the `badge-generator` plugin's `:metric` handler with the
///    target user's context (plugins see `ctx.user_id` as that user)  
/// 4. Converts the plugin's JSON response (`{ label, message, color }`) into
///    a flat SVG badge, or forwards a raw SVG string directly  
pub async fn get_badge(
    State(state): State<AppState>,
    Path((username, metric)): Path<(String, String)>,
    Query(params): Query<BadgeQuery>,
) -> impl IntoResponse {
    // ── 1. Sanitise the metric name (same rules as handler_safe in rpc_call) ──
    let metric_safe: String = metric
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();

    if metric_safe.is_empty() {
        return svg_response(error_badge_svg("invalid metric"), StatusCode::BAD_REQUEST);
    }

    // ── 2. Resolve username → user record ────────────────────────────────────
    #[derive(sqlx::FromRow)]
    struct UserRow {
        id: uuid::Uuid,
        is_public: bool,
    }

    let user = match sqlx::query_as::<_, UserRow>(
        "SELECT id, is_public FROM users WHERE username = $1 LIMIT 1",
    )
    .bind(&username)
    .fetch_optional(&state.db.pool)
    .await
    {
        Ok(Some(u)) => u,
        Ok(None) => {
            // Same response as private — no enumeration
            return svg_response(private_badge_svg(), StatusCode::OK);
        }
        Err(e) => {
            tracing::warn!("[badge] DB error looking up '{}': {}", username, e);
            return svg_response(error_badge_svg("service error"), StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // ── 3. Enforce public profile ─────────────────────────────────────────────
    if !user.is_public {
        return svg_response(private_badge_svg(), StatusCode::OK);
    }

    // ── 4. Load the badge-generator plugin (must be published, not banned) ────
    #[derive(sqlx::FromRow)]
    struct PluginRow {
        script: Option<String>,
        id: uuid::Uuid,
    }

    // The plugin doesn't need to be installed by the target user — it is a
    // *public system plugin*. We just need it to exist and be published.
    let plugin = match sqlx::query_as::<_, PluginRow>(
        r#"SELECT script, id
           FROM plugin_store
           WHERE name = 'badge-generator'
             AND is_published = true
             AND is_banned    = false
           LIMIT 1"#,
    )
    .fetch_optional(&state.db.pool)
    .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return svg_response(
                error_badge_svg("badge-generator not found"),
                StatusCode::NOT_FOUND,
            );
        }
        Err(e) => {
            tracing::warn!("[badge] DB error loading plugin: {}", e);
            return svg_response(error_badge_svg("service error"), StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let script = match plugin.script {
        Some(s) => s,
        None => return svg_response(error_badge_svg("plugin has no script"), StatusCode::BAD_REQUEST),
    };

    // ── 5. Build request JSON forwarded to the plugin handler ─────────────────
    let req_json = serde_json::json!({
        "style": params.style.as_deref().unwrap_or("flat"),
        "username": username,
        "metric": metric_safe,
    })
    .to_string();

    // ── 6. Run the plugin in QuickJS with the TARGET user's context ───────────
    //
    // We pass target `user.id` as the `user_id` — the plugin sees
    // `ctx.user_id` as the profile owner, NOT the anonymous caller.
    // This is intentional: the plugin reads data about that user.
    let user_id_str = user.id.to_string();
    let db_pool     = state.db.pool.clone();
    let redis_pool  = state.redis.clone();
    let plugin_id   = plugin.id;
    let metric_log  = metric_safe.clone();

    let plugin_result = tokio::time::timeout(
        std::time::Duration::from_secs(10), // tighter than RPC (public endpoint)
        tokio::task::spawn_blocking(move || {
            run_rpc_in_quickjs(
                &script,
                &metric_safe,
                "badge-generator",
                &user_id_str,
                &req_json,
                db_pool,
                redis_pool,
                plugin_id,
            )
        }),
    )
    .await;

    let value = match plugin_result {
        Ok(Ok(Ok(v))) => v,
        Ok(Ok(Err(err))) => {
            tracing::warn!("[badge] plugin handler '{}' failed: {}", metric_log, err);
            return svg_response(error_badge_svg("handler error"), StatusCode::INTERNAL_SERVER_ERROR);
        }
        Ok(Err(join_err)) => {
            tracing::warn!("[badge] task join error: {}", join_err);
            return svg_response(error_badge_svg("handler error"), StatusCode::INTERNAL_SERVER_ERROR);
        }
        Err(_) => {
            return svg_response(error_badge_svg("timeout"), StatusCode::GATEWAY_TIMEOUT);
        }
    };

    // ── 7. Convert plugin result → SVG ────────────────────────────────────────
    //
    // Convention A: plugin returns { label, message, color } → server renders SVG.
    // Convention B: plugin returns { svg: "<svg>…</svg>" } → forwarded as-is.
    if let Some(raw_svg) = value.get("svg").and_then(|v| v.as_str()) {
        // Plugin generated its own SVG — forward directly (it's the plugin
        // author's responsibility to escape content correctly).
        return svg_response(raw_svg.to_string(), StatusCode::OK);
    }

    let label   = value.get("label")  .and_then(|v| v.as_str()).unwrap_or("codetrackr");
    let message = value.get("message").and_then(|v| v.as_str()).unwrap_or("no data");
    let color   = value.get("color")  .and_then(|v| v.as_str()).unwrap_or("#4c9be8");

    svg_response(render_flat_svg(label, message, color), StatusCode::OK)
}
