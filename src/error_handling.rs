use axum::{
    http::StatusCode,
    Json,
};
use serde_json::json;
use tracing::error;

/// Maneja errores de base de datos de forma segura: loguea el error real pero devuelve mensaje genérico al cliente
pub fn handle_database_error(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    // Loguear el error completo para depuración interna
    error!("Database error: {}", e);
    
    // Devolver mensaje genérico al cliente para no exponer información sensible
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "Internal server error"}))
    )
}

/// Maneja errores de autenticación de forma segura
pub fn handle_auth_error(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    error!("Auth error: {}", e);
    
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "Authentication failed"}))
    )
}

/// Maneja errores generales de forma segura
#[allow(dead_code)]
pub fn handle_general_error(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    error!("General error: {}", e);
    
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "Internal server error"}))
    )
}
