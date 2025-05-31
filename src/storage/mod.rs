use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePool, Row, Sqlite};
use std::sync::Arc;
use std::path::Path;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::types::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameResult {
    pub room_id: String,
    pub winner: Option<PlayerId>,
    pub duration_seconds: u64,
    pub final_scores: (u32, u32),
    pub final_canvas_size: (u32, u32),
    pub total_clicks: u64,
    pub powerups_used: u32,
    pub end_reason: EndReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStats {
    pub player_name: String,
    pub total_games: u32,
    pub games_won: u32,
    pub total_clicks: u64,
    pub favorite_powerup: Option<String>,
    pub average_game_duration: f64,
}

pub struct Database {
    pool: SqlitePool,
    sync_queue: Arc<Mutex<Vec<SyncOperation>>>,
}

#[derive(Debug)]
pub enum SyncOperation {
    GameCompleted(GameResult),
    PlayerStatsUpdate(PlayerStats),
    CanvasSnapshot { room_id: String, canvas_data: Vec<u8> },
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        // Create directory if it doesn't exist (for file-based SQLite databases)
        if database_url.starts_with("sqlite:") {
            let file_path = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
            info!("Database file path: {}", file_path);
            
            if let Some(parent) = Path::new(file_path).parent() {
                if !parent.exists() {
                    info!("Creating database directory: {:?}", parent);
                    std::fs::create_dir_all(parent)
                        .map_err(|e| anyhow::anyhow!("Failed to create database directory {:?}: {}", parent, e))?;
                    info!("Created database directory: {:?}", parent);
                } else {
                    info!("Database directory already exists: {:?}", parent);
                }
            }
        }
        
        info!("Attempting to connect to database: {}", database_url);
        
        // For SQLite, add connection options to ensure proper file creation
        let connection_url = if database_url.starts_with("sqlite:") {
            format!("{}?mode=rwc", database_url) // rwc = read-write-create
        } else {
            database_url.to_string()
        };
        
        info!("Full connection URL: {}", connection_url);
        let pool = SqlitePool::connect(&connection_url).await
            .map_err(|e| anyhow::anyhow!("Failed to connect to database {}: {}", connection_url, e))?;
        
        info!("Database connection successful, running migrations...");
        // Run migrations - comment this out temporarily if migrations directory doesn't exist
        // sqlx::migrate!("./migrations").run(&pool).await
        //     .map_err(|e| anyhow::anyhow!("Failed to run migrations: {}", e))?;
        
        let database = Self {
            pool,
            sync_queue: Arc::new(Mutex::new(Vec::new())),
        };
        
        info!("Database initialization complete");
        // Start background sync task
        database.start_background_sync().await;
        
        Ok(database)
    }

    pub async fn save_game_result(&self, game_result: GameResult) -> Result<()> {
        let mut queue = self.sync_queue.lock().await;
        queue.push(SyncOperation::GameCompleted(game_result));
        Ok(())
    }

    pub async fn update_player_stats(&self, stats: PlayerStats) -> Result<()> {
        let mut queue = self.sync_queue.lock().await;
        queue.push(SyncOperation::PlayerStatsUpdate(stats));
        Ok(())
    }

    pub async fn save_canvas_snapshot(&self, room_id: String, canvas_data: Vec<u8>) -> Result<()> {
        let mut queue = self.sync_queue.lock().await;
        queue.push(SyncOperation::CanvasSnapshot { room_id, canvas_data });
        Ok(())
    }

    pub async fn get_player_stats(&self, player_name: &str) -> Result<Option<PlayerStats>> {
        let row = sqlx::query(
            "SELECT player_name, total_games, games_won, total_clicks, favorite_powerup, average_game_duration 
             FROM player_stats WHERE player_name = ?"
        )
        .bind(player_name)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(PlayerStats {
                player_name: row.get("player_name"),
                total_games: row.get("total_games"),
                games_won: row.get("games_won"),
                total_clicks: row.get("total_clicks"),
                favorite_powerup: row.get("favorite_powerup"),
                average_game_duration: row.get("average_game_duration"),
            }))
        } else {
            Ok(None)
        }
    }

    async fn start_background_sync(&self) {
        let pool = self.pool.clone();
        let sync_queue = Arc::clone(&self.sync_queue);
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            
            loop {
                interval.tick().await;
                
                let operations = {
                    let mut queue = sync_queue.lock().await;
                    if queue.is_empty() {
                        continue;
                    }
                    std::mem::take(&mut *queue)
                };
                
                for operation in operations {
                    if let Err(e) = Self::process_sync_operation(&pool, operation).await {
                        error!("Failed to process sync operation: {}", e);
                    }
                }
            }
        });
    }

    async fn process_sync_operation(pool: &SqlitePool, operation: SyncOperation) -> Result<()> {
        match operation {
            SyncOperation::GameCompleted(game_result) => {
                Self::insert_game_result(pool, game_result).await?;
            }
            SyncOperation::PlayerStatsUpdate(stats) => {
                Self::upsert_player_stats(pool, stats).await?;
            }
            SyncOperation::CanvasSnapshot { room_id, canvas_data } => {
                Self::insert_canvas_snapshot(pool, room_id, canvas_data).await?;
            }
        }
        Ok(())
    }

    async fn insert_game_result(pool: &SqlitePool, game_result: GameResult) -> Result<()> {
        sqlx::query(
            "INSERT INTO game_results 
             (room_id, winner, duration_seconds, final_score_0, final_score_1, 
              final_width, final_height, total_clicks, powerups_used, end_reason, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))"
        )
        .bind(&game_result.room_id)
        .bind(game_result.winner.map(|w| w as i32))
        .bind(game_result.duration_seconds as i64)
        .bind(game_result.final_scores.0 as i64)
        .bind(game_result.final_scores.1 as i64)
        .bind(game_result.final_canvas_size.0 as i64)
        .bind(game_result.final_canvas_size.1 as i64)
        .bind(game_result.total_clicks as i64)
        .bind(game_result.powerups_used as i64)
        .bind(serde_json::to_string(&game_result.end_reason)?)
        .execute(pool)
        .await?;

        info!("Saved game result for room {}", game_result.room_id);
        Ok(())
    }

    async fn upsert_player_stats(pool: &SqlitePool, stats: PlayerStats) -> Result<()> {
        sqlx::query(
            "INSERT INTO player_stats 
             (player_name, total_games, games_won, total_clicks, favorite_powerup, average_game_duration, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, datetime('now'))
             ON CONFLICT(player_name) DO UPDATE SET
             total_games = excluded.total_games,
             games_won = excluded.games_won,
             total_clicks = excluded.total_clicks,
             favorite_powerup = excluded.favorite_powerup,
             average_game_duration = excluded.average_game_duration,
             updated_at = datetime('now')"
        )
        .bind(&stats.player_name)
        .bind(stats.total_games as i64)
        .bind(stats.games_won as i64)
        .bind(stats.total_clicks as i64)
        .bind(&stats.favorite_powerup)
        .bind(stats.average_game_duration)
        .execute(pool)
        .await?;

        Ok(())
    }

    async fn insert_canvas_snapshot(pool: &SqlitePool, room_id: String, canvas_data: Vec<u8>) -> Result<()> {
        sqlx::query(
            "INSERT INTO canvas_snapshots (room_id, canvas_data, created_at) 
             VALUES (?, ?, datetime('now'))"
        )
        .bind(&room_id)
        .bind(&canvas_data)
        .execute(pool)
        .await?;

        Ok(())
    }
}