/// Content Security Policy (CSP) Middleware
///
/// Implements CSP headers for security:
/// - Restricts script sources to prevent XSS
/// - Controls object sources, frame ancestors
/// - Defines default-src, connect-src, img-src policies
/// - Supports development vs production policies

use axum::{
    extract::Request,
    http::{StatusCode, header},
    middleware::Next,
    response::Response,
};
use std::env;

/// CSP configuration
#[derive(Debug, Clone)]
pub struct CspConfig {
    pub development: bool,
    pub frontend_url: String,
}

impl Default for CspConfig {
    fn default() -> Self {
        Self {
            development: env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string()) == "development",
            frontend_url: env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
        }
    }
}

/// Builds CSP header value based on configuration
fn build_csp_header(config: &CspConfig) -> String {
    if config.development {
        // Development: more permissive for local development
        format!(
            concat!(
                "default-src 'self'; ",
                "script-src 'self' 'unsafe-inline' 'unsafe-eval' ws: wss: https://cdnjs.cloudflare.com https://cdn.jsdelivr.net {}; ",
                "style-src 'self' 'unsafe-inline' https://fonts.googleapis.com https://cdnjs.cloudflare.com; ",
                "img-src 'self' data: https:; ",
                "font-src 'self' data: https://fonts.gstatic.com; ",
                "connect-src 'self' ws: wss: {} https://api.github.com https://gitlab.com; ",
                "object-src 'none'; ",
                "base-uri 'self'; ",
                "form-action 'self'; ",
                "frame-ancestors 'none'; "
            ),
            config.frontend_url,
            config.frontend_url
        )
    } else {
        // Production: strict CSP
        format!(
            concat!(
                "default-src 'self'; ",
                "script-src 'self' https://cdnjs.cloudflare.com https://cdn.jsdelivr.net {}; ",
                "style-src 'self' 'unsafe-inline' https://fonts.googleapis.com https://cdnjs.cloudflare.com; ",
                "img-src 'self' data: https:; ",
                "font-src 'self' data: https://fonts.gstatic.com; ",
                "connect-src 'self' {} https://api.github.com https://gitlab.com; ",
                "object-src 'none'; ",
                "base-uri 'self'; ",
                "form-action 'self'; ",
                "frame-ancestors 'none'; ",
                "upgrade-insecure-requests;"
            ),
            config.frontend_url,
            config.frontend_url
        )
    }
}

/// CSP middleware layer
pub async fn csp_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let config = CspConfig::default();
    let csp_value = build_csp_header(&config);
    
    let mut response = next.run(request).await;
    
    // Add CSP header
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        csp_value.parse().expect("Invalid CSP header value"),
    );
    
    // Add other security headers
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    
    response.headers_mut().insert(
        header::X_FRAME_OPTIONS,
        "DENY".parse().unwrap(),
    );
    
    response.headers_mut().insert(
        header::X_XSS_PROTECTION,
        "1; mode=block".parse().unwrap(),
    );
    
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    
    if !config.development {
        response.headers_mut().insert(
            header::STRICT_TRANSPORT_SECURITY,
            "max-age=31536000; includeSubDomains".parse().unwrap(),
        );
    }
    
    Ok(response)
}

/// CSP middleware for API endpoints (more restrictive)
pub async fn api_csp_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let mut response = next.run(request).await;
    
    // API endpoints only need basic security headers
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    
    response.headers_mut().insert(
        header::X_FRAME_OPTIONS,
        "DENY".parse().unwrap(),
    );
    
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    
    Ok(response)
}
