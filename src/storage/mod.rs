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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Room {
    pub id: i64,
    pub room_code: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub status: String,
    pub settings_json: Option<String>,
    pub winner_user_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RoomPlayer {
    pub id: i64,
    pub room_id: i64,
    pub user_id: Option<i64>,
    pub player_name: String,
    pub player_index: i32,
    pub joined_at: String,
    pub left_at: Option<String>,
    pub is_guest: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Move {
    pub id: i64,
    pub room_id: i64,
    pub player_id: i64,
    pub move_type: String,
    pub move_data_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PowerupUsage {
    pub id: i64,
    pub room_id: i64,
    pub player_id: i64,
    pub powerup_type: String,
    pub used_at: String,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CustomAbility {
    pub id: i64,
    pub room_id: i64,
    pub player_id: i64,
    pub ability_name: String,
    pub ability_data_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AbilityUsage {
    pub id: i64,
    pub room_id: i64,
    pub player_id: i64,
    pub ability_id: i64,
    pub used_at: String,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CommentaryLog {
    pub id: i64,
    pub room_id: i64,
    pub event_type: String,
    pub message: String,
    pub triggered_by_player_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RoomStats {
    pub room_id: i64,
    pub total_moves: Option<i64>,
    pub total_powerups: Option<i64>,
    pub total_abilities: Option<i64>,
    pub duration_seconds: Option<i64>,
    pub winner_player_id: Option<i64>,
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
        sqlx::migrate!("./migrations").run(&pool).await
            .map_err(|e| anyhow::anyhow!("Failed to run migrations: {}", e))?;
        
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

    pub async fn insert_user(&self, name: &str, email: Option<&str>, password_hash: Option<&str>) -> Result<i64> {
        let rec = sqlx::query(
            "INSERT INTO users (name, email, password_hash) VALUES (?, ?, ?)"
        )
        .bind(name)
        .bind(email)
        .bind(password_hash)
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }

    pub async fn get_user_by_id(&self, user_id: i64) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    // Room helpers
    pub async fn insert_room(&self, room_code: &str, status: &str, settings_json: Option<&str>) -> Result<i64> {
        let rec = sqlx::query(
            "INSERT INTO rooms (room_code, status, settings_json) VALUES (?, ?, ?)"
        )
        .bind(room_code)
        .bind(status)
        .bind(settings_json)
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }
    pub async fn get_room_by_id(&self, room_id: i64) -> Result<Option<Room>> {
        let row = sqlx::query_as::<_, Room>("SELECT * FROM rooms WHERE id = ?")
            .bind(room_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }
    // RoomPlayer helpers
    pub async fn insert_room_player(&self, room_id: i64, user_id: Option<i64>, player_name: &str, player_index: i32, is_guest: bool) -> Result<i64> {
        let rec = sqlx::query(
            "INSERT INTO room_players (room_id, user_id, player_name, player_index, is_guest) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(room_id)
        .bind(user_id)
        .bind(player_name)
        .bind(player_index)
        .bind(is_guest)
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }
    pub async fn get_room_player_by_id(&self, player_id: i64) -> Result<Option<RoomPlayer>> {
        let row = sqlx::query_as::<_, RoomPlayer>("SELECT * FROM room_players WHERE id = ?")
            .bind(player_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }
    // Move helpers
    pub async fn insert_move(&self, room_id: i64, player_id: i64, move_type: &str, move_data_json: Option<&str>) -> Result<i64> {
        let rec = sqlx::query(
            "INSERT INTO moves (room_id, player_id, move_type, move_data_json) VALUES (?, ?, ?, ?)"
        )
        .bind(room_id)
        .bind(player_id)
        .bind(move_type)
        .bind(move_data_json)
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }
    pub async fn get_move_by_id(&self, move_id: i64) -> Result<Option<Move>> {
        let row = sqlx::query_as::<_, Move>("SELECT * FROM moves WHERE id = ?")
            .bind(move_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }
    // PowerupUsage helpers
    pub async fn insert_powerup_usage(&self, room_id: i64, player_id: i64, powerup_type: &str, result: Option<&str>) -> Result<i64> {
        let rec = sqlx::query(
            "INSERT INTO powerup_usage (room_id, player_id, powerup_type, result) VALUES (?, ?, ?, ?)"
        )
        .bind(room_id)
        .bind(player_id)
        .bind(powerup_type)
        .bind(result)
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }
    pub async fn get_powerup_usage_by_id(&self, id: i64) -> Result<Option<PowerupUsage>> {
        let row = sqlx::query_as::<_, PowerupUsage>("SELECT * FROM powerup_usage WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }
    // CustomAbility helpers
    pub async fn insert_custom_ability(&self, room_id: i64, player_id: i64, ability_name: &str, ability_data_json: Option<&str>) -> Result<i64> {
        let rec = sqlx::query(
            "INSERT INTO custom_abilities (room_id, player_id, ability_name, ability_data_json) VALUES (?, ?, ?, ?)"
        )
        .bind(room_id)
        .bind(player_id)
        .bind(ability_name)
        .bind(ability_data_json)
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }
    pub async fn get_custom_ability_by_id(&self, id: i64) -> Result<Option<CustomAbility>> {
        let row = sqlx::query_as::<_, CustomAbility>("SELECT * FROM custom_abilities WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }
    // AbilityUsage helpers
    pub async fn insert_ability_usage(&self, room_id: i64, player_id: i64, ability_id: i64, result: Option<&str>) -> Result<i64> {
        let rec = sqlx::query(
            "INSERT INTO ability_usage (room_id, player_id, ability_id, result) VALUES (?, ?, ?, ?)"
        )
        .bind(room_id)
        .bind(player_id)
        .bind(ability_id)
        .bind(result)
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }
    pub async fn get_ability_usage_by_id(&self, id: i64) -> Result<Option<AbilityUsage>> {
        let row = sqlx::query_as::<_, AbilityUsage>("SELECT * FROM ability_usage WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }
    // CommentaryLog helpers
    pub async fn insert_commentary_log(&self, room_id: i64, event_type: &str, message: &str, triggered_by_player_id: Option<i64>) -> Result<i64> {
        let rec = sqlx::query(
            "INSERT INTO commentary_log (room_id, event_type, message, triggered_by_player_id) VALUES (?, ?, ?, ?)"
        )
        .bind(room_id)
        .bind(event_type)
        .bind(message)
        .bind(triggered_by_player_id)
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }
    pub async fn get_commentary_log_by_id(&self, id: i64) -> Result<Option<CommentaryLog>> {
        let row = sqlx::query_as::<_, CommentaryLog>("SELECT * FROM commentary_log WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }
    // RoomStats helpers
    pub async fn insert_room_stats(&self, room_id: i64, total_moves: Option<i64>, total_powerups: Option<i64>, total_abilities: Option<i64>, duration_seconds: Option<i64>, winner_player_id: Option<i64>) -> Result<i64> {
        let rec = sqlx::query(
            "INSERT INTO room_stats (room_id, total_moves, total_powerups, total_abilities, duration_seconds, winner_player_id) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(room_id)
        .bind(total_moves)
        .bind(total_powerups)
        .bind(total_abilities)
        .bind(duration_seconds)
        .bind(winner_player_id)
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }
    pub async fn get_room_stats_by_id(&self, room_id: i64) -> Result<Option<RoomStats>> {
        let row = sqlx::query_as::<_, RoomStats>("SELECT * FROM room_stats WHERE room_id = ?")
            .bind(room_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }
}