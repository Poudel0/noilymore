#![allow(warnings)]
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, Json},
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::{fs, net::TcpListener};
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

mod config;
mod game;
mod storage;
mod types;
mod ws;

use config::Config;
use game::GameManager;
use storage::Database;
use types::*;

#[derive(Clone)]
pub struct AppState {
    pub game_manager: Arc<GameManager>,
    pub database: Arc<Database>,
    pub config: Arc<Config>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    let config = Arc::new(Config::new());
    let database = Arc::new(Database::new(&config.database_url).await?);
    let game_manager = Arc::new(GameManager::new(database.clone()));
    
    let state = AppState {
        game_manager,
        database,
        config: config.clone(),
    };

    let app = Router::new()
        .route("/", get(serve_frontend))
        .route("/health", get(health_handler))
        .route("/rooms", post(create_room_handler))
        .route("/rooms/{room_id}", get(get_room_handler))
        .route("/ws/{room_id}", get(ws::websocket_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    info!("Canvas Wars server starting on {}", addr);
    info!("Game available at: http://{}", addr);
    
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}

// Serve the frontend HTML file
async fn serve_frontend() -> Result<Html<String>, StatusCode> {
    match fs::read_to_string("index.html").await {
        Ok(content) => Ok(Html(content)),
        Err(e) => {
            warn!("Failed to read index.html: {}", e);
            Err(StatusCode::NOT_FOUND)
        }
    }
}

async fn health_handler() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "canvas-wars",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn create_room_handler(
    State(state): State<AppState>,
) -> Result<Json<CreateRoomResponse>, StatusCode> {
    match state.game_manager.create_room().await {
        Ok(room_id) => Ok(Json(CreateRoomResponse { room_id })),
        Err(e) => {
            warn!("Failed to create room: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_room_handler(
    State(state): State<AppState>,
    axum::extract::Path(room_id): axum::extract::Path<String>,
) -> Result<Json<GameState>, StatusCode> {
    match state.game_manager.get_room_state(&room_id).await {
        Some(state) => Ok(Json(state)),
        None => Err(StatusCode::NOT_FOUND),
    }
}