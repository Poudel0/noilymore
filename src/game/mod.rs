use anyhow::Result;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;
use tokio::sync::mpsc::UnboundedSender;


use crate::config::Config;
use crate::storage::Database;
use crate::types::*;

mod powerups;
mod canvas_growth;

use powerups::PowerupSystem;
use canvas_growth::CanvasGrowthSystem;


#[derive(Clone)]
pub struct ConnectionHandle {
    pub player_id: PlayerId,
    pub sender: UnboundedSender<ServerMessage>,
}

pub struct GameManager {
    pub rooms: Arc<DashMap<String, Arc<RwLock<ActiveRoom>>>>,
    connections: Arc<DashMap<String, Vec<ConnectionHandle>>>,
    database: Arc<Database>,
    powerup_system: PowerupSystem,
    growth_system: CanvasGrowthSystem,
    config: Arc<Config>,
}

pub struct ActiveRoom {
    pub room_id: String, // room_code
    pub db_room_id: Option<i64>, // new: DB id
    pub canvas: Canvas,
    pub players: [Option<PlayerState>; 2],
    pub db_player_ids: [Option<i64>; 2], // new: DB player ids
    pub powerup_manager: PowerupManager,
    pub game_status: GameStatus,
    pub last_update: std::time::SystemTime,
    pub growth_tracker: CanvasGrowth,
}

impl GameManager {
    pub fn new(database: Arc<Database>, config: Arc<Config>) -> Self {
        let rooms = Arc::new(DashMap::new());
        
        // Start cleanup task
        let cleanup_rooms = Arc::clone(&rooms);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                Self::cleanup_inactive_rooms(&cleanup_rooms).await;
            }
        });

        Self {
            rooms,
            connections: Arc::new(DashMap::new()),
            database,
            powerup_system: PowerupSystem::new(),
            growth_system: CanvasGrowthSystem::new(),
            config,
        }
    }

    pub async fn create_room(&self) -> Result<String> {
        let room_code = Uuid::new_v4().to_string().chars()
        .filter(|c| c.is_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_uppercase();
        let db_room_id = self.database.insert_room(&room_code, "waiting", None).await?;
        let (width, height) = self.config.canvas_config.initial_size;
        let mut canvas = Canvas::new(width, height);
        let total_cells = (width * height) as usize;
        let half = total_cells / 2;
        let mut assignments = vec![0u8; half];
        assignments.extend(vec![1u8; half]);
        if assignments.len() < total_cells {
            assignments.push(255u8); // 255 = unclaimed
        }
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        assignments.shuffle(&mut rng);
        for (i, value) in assignments.into_iter().enumerate() {
            let x = (i as u32) % width;
            let y = (i as u32) / width;
            if value == 0 {
                canvas.set_cell(x, y, 0);
            } else if value == 1 {
                canvas.set_cell(x, y, 1);
            } // else leave unclaimed
        }
        let room = ActiveRoom {
            room_id: room_code.clone(),
            db_room_id: Some(db_room_id),
            canvas,
            players: [None, None],
            db_player_ids: [None, None],
            powerup_manager: PowerupManager::new(),
            game_status: GameStatus::WaitingForPlayers,
            last_update: std::time::SystemTime::now(),
            growth_tracker: CanvasGrowth {
                last_expansion: Instant::now(),
                current_size: (width, height),
                expansion_count: 0,
            },
        };
        self.rooms.insert(room_code.clone(), Arc::new(RwLock::new(room)));
        info!("Created room: {} (db id: {})", room_code, db_room_id);
        Ok(room_code)
    }

    pub async fn join_room(&self, room_code: &str, player_name: String) -> Result<(PlayerId, GameState)> {
        let room_lock = self.rooms.get(room_code)
            .ok_or_else(|| anyhow::anyhow!("Room not found"))?;
        let mut room = room_lock.write().await;
        let player_id = if room.players[0].is_none() {
            0
        } else if room.players[1].is_none() {
            1
        } else {
            return Err(anyhow::anyhow!("Room is full"));
        };
        // Insert into room_players table
        let db_room_id = room.db_room_id.ok_or_else(|| anyhow::anyhow!("Room DB id missing"))?;
        let db_player_id = self.database.insert_room_player(db_room_id, None, &player_name, player_id, true).await?;
        // Store DB player id
        room.db_player_ids[player_id as usize] = Some(db_player_id);
        let player_state = PlayerState {
            id: player_id as u8,
            name: player_name,
            score: 0,
            powerup_cooldowns: std::collections::HashMap::new(),
            joined_at: chrono::Utc::now().timestamp() as u64,
        };
        room.players[player_id as usize] = Some(player_state.clone());
        room.last_update = std::time::SystemTime::now();
        // Start game if both players joined
        if room.players[0].is_some() && room.players[1].is_some() {
            room.game_status = GameStatus::Active;
            info!("Game started in room: {}", room_code);
            self.broadcast_to_room(room_code, ServerMessage::GameStarted).await?;
        }
        let game_state = self.room_to_game_state(&room);
        Ok((player_id as u8, game_state))
    }

    pub async fn get_room_state(&self, room_id: &str) -> Option<GameState> {
        let room_lock = self.rooms.get(room_id)?;
        let room = room_lock.read().await;
        Some(self.room_to_game_state(&room))
    }

    pub async fn process_click_batch(
        &self,
        room_id: &str,
        player_id: PlayerId,
        coordinates: Vec<(u32, u32)>,
        sequence_id: u32,
    ) -> Result<Vec<GridChange>> {
        let room_lock = self.rooms.get(room_id)
            .ok_or_else(|| anyhow::anyhow!("Room not found"))?;
        
        let mut room = room_lock.write().await;
        
        // Validate game is active
        if !matches!(room.game_status, GameStatus::Active) {
            return Err(anyhow::anyhow!("Game is not active"));
        }

        let mut changes = Vec::new();
        
        for (x, y) in coordinates {
            if x >= room.canvas.width || y >= room.canvas.height {
                continue; // Skip invalid coordinates
            }

            let old_owner = room.canvas.get_cell(x, y);
            
            // Only allow claiming empty cells or stealing from opponent
            if old_owner != Some(player_id) {
                room.canvas.set_cell(x, y, player_id);
                
                changes.push(GridChange {
                    x,
                    y,
                    old_owner,
                    new_owner: player_id,
                });
                // Log move to DB
                if let (Some(db_room_id), Some(db_player_id)) = (room.db_room_id, room.db_player_ids[player_id as usize]) {
                    let move_data = serde_json::json!({"x": x, "y": y, "old_owner": old_owner, "new_owner": player_id});
                    let _ = self.database.insert_move(db_room_id, db_player_id, "tap", Some(&move_data.to_string())).await;
                }
            }
        }

        // Update scores
        let (score_0, score_1) = room.canvas.get_scores();
        if let Some(ref mut player) = room.players[0] {
            player.score = score_0;
        }
        if let Some(ref mut player) = room.players[1] {
            player.score = score_1;
        }

        // Check for canvas growth
        if self.growth_system.should_expand(&room.canvas, &room.growth_tracker) {
            let new_size = self.growth_system.calculate_new_size(&room.growth_tracker);
            self.expand_canvas(&mut room, new_size).await?;
        }

        room.last_update = std::time::SystemTime::now();
        
        Ok(changes)
    }

    pub async fn activate_powerup(
        &self,
        room_id: &str,
        player_id: PlayerId,
        powerup_id: &str,
    ) -> Result<PowerupResult> {
        let room_lock = self.rooms.get(room_id)
            .ok_or_else(|| anyhow::anyhow!("Room not found"))?;
        
        let mut room = room_lock.write().await;
        
        if !matches!(room.game_status, GameStatus::Active) {
            return Ok(PowerupResult::GameNotActive);
        }

        if !room.powerup_manager.can_activate(powerup_id, player_id) {
            return Ok(PowerupResult::OnCooldown);
        }

        let result = self.powerup_system.execute_powerup(
            powerup_id,
            player_id,
            &mut *room,
        ).await?;

        room.last_update = std::time::SystemTime::now();
        // Log powerup usage
        if let (Some(db_room_id), Some(db_player_id)) = (room.db_room_id, room.db_player_ids[player_id as usize]) {
            let _ = self.database.insert_powerup_usage(db_room_id, db_player_id, powerup_id, Some(&format!("{:?}", result))).await;
            // Log commentary
            let msg = format!("Player {} used powerup {}", player_id, powerup_id);
            let _ = self.database.insert_commentary_log(db_room_id, "powerup", &msg, Some(db_player_id)).await;
        }
        Ok(result)
    }

    pub async fn accept_defeat(&self, room_id: &str, player_id: PlayerId) -> Result<()> {
        let room_lock = self.rooms.get(room_id)
            .ok_or_else(|| anyhow::anyhow!("Room not found"))?;
        
        let mut room = room_lock.write().await;
        
        if !matches!(room.game_status, GameStatus::Active) {
            return Err(anyhow::anyhow!("Game is not active"));
        }

        let winner = if player_id == 0 { Some(1) } else { Some(0) };
        room.game_status = GameStatus::Ended {
            winner,
            reason: EndReason::PlayerSurrender,
        };

        // Save game result
        self.save_game_result(&room).await?;
        
        // Log commentary and stats
        if let Some(db_room_id) = room.db_room_id {
            let msg = format!("Player {} surrendered", player_id);
            let _ = self.database.insert_commentary_log(db_room_id, "surrender", &msg, room.db_player_ids[player_id as usize]).await;
            // Insert room_stats (basic example)
            let (score_0, score_1) = room.canvas.get_scores();
            let total_moves = (score_0 + score_1) as i64;
            let winner_db_id = winner.and_then(|w| room.db_player_ids[w as usize]);
            let _ = self.database.insert_room_stats(db_room_id, Some(total_moves), None, None, None, winner_db_id).await;
        }
        
        info!("Player {} surrendered in room {}", player_id, room_id);
        
        Ok(())
    }

    async fn expand_canvas(&self, room: &mut ActiveRoom, new_size: (u32, u32)) -> Result<()> {
        let old_canvas = &room.canvas;
        let mut new_canvas = Canvas::new(new_size.0, new_size.1);
        
        // Copy existing data to center of new canvas
        let offset_x = (new_size.0 - old_canvas.width) / 2;
        let offset_y = (new_size.1 - old_canvas.height) / 2;
        
        for y in 0..old_canvas.height {
            for x in 0..old_canvas.width {
                if let Some(owner) = old_canvas.get_cell(x, y) {
                    new_canvas.set_cell(x + offset_x, y + offset_y, owner);
                }
            }
        }
        
        room.canvas = new_canvas;
        room.growth_tracker.current_size = new_size;
        room.growth_tracker.last_expansion = Instant::now();
        room.growth_tracker.expansion_count += 1;
        
        info!("Canvas expanded to {:?} in room {}", new_size, room.room_id);
        
        Ok(())
    }

    async fn save_game_result(&self, room: &ActiveRoom) -> Result<()> {
        let duration = room.last_update.duration_since(
            room.players.iter()
                .flatten()
                .map(|p| std::time::UNIX_EPOCH + std::time::Duration::from_secs(p.joined_at))
                .min()
                .unwrap_or(std::time::UNIX_EPOCH)
        );

        let (winner, reason) = match &room.game_status {
            GameStatus::Ended { winner, reason } => (*winner, reason.clone()),
            _ => (None, EndReason::Timeout),
        };

        let game_result = crate::storage::GameResult {
            room_id: room.room_id.clone(),
            winner,
            duration_seconds: duration?.as_secs(),
            final_scores: room.canvas.get_scores(),
            final_canvas_size: (room.canvas.width, room.canvas.height),
            total_clicks: (room.canvas.get_scores().0 + room.canvas.get_scores().1) as u64,
            powerups_used: 0, // TODO: Track powerup usage
            end_reason: reason,
        };

        self.database.save_game_result(game_result).await?;
        
        Ok(())
    }

    fn room_to_game_state(&self, room: &ActiveRoom) -> GameState {
        
        GameState {
            room_id: room.room_id.clone(),
            canvas_size: (room.canvas.width, room.canvas.height),
            scores: room.canvas.get_scores(),
            players: room.players.clone(),
            game_status: room.game_status.clone(),
            powerups_available: room.powerup_manager.available_powerups.clone(),
        }
    }

    
        pub fn get_flattened_grid_from_state(&self, state: &GameState) -> Vec<u8> {
            // Create a temporary canvas for flattening based on current size
            let canvas = Canvas::new(state.canvas_size.0, state.canvas_size.1);
    
            // Replace this if you want to map from actual canvas (ActiveRoom)
            self.get_flattened_grid(&canvas)
        }
    
    

     pub fn get_flattened_grid(&self, canvas: &Canvas) -> Vec<u8> {
        let mut grid = Vec::with_capacity((canvas.width * canvas.height) as usize);

        for y in 0..canvas.height {
            for x in 0..canvas.width {
                match canvas.get_cell(x, y) {
                    Some(0) => grid.push(0),   // Player 0
                    Some(1) => grid.push(1),   // Player 1
                    _ => grid.push(255),    // Unclaimed cell
                }
            }
        }

        grid
    }

    async fn cleanup_inactive_rooms(rooms: &DashMap<String, Arc<RwLock<ActiveRoom>>>) {
        let mut to_remove = Vec::new();
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(1800); // 30 minutes
        
        for entry in rooms.iter() {
            let room = entry.value().read().await;
            if room.last_update < cutoff || matches!(room.game_status, GameStatus::Ended { .. }) {
                to_remove.push(entry.key().clone());
            }
        }
        
        for room_id in to_remove {
            rooms.remove(&room_id);
            info!("Cleaned up inactive room: {}", room_id);
        }
    }
    pub fn register_connection(
        &self,
        room_id: String,
        player_id: PlayerId,
        sender: UnboundedSender<ServerMessage>,
    ) {
        self.connections
            .entry(room_id)
            .or_default()
            .push(ConnectionHandle { player_id, sender });
    }

    pub async fn broadcast_to_room(
        &self,
        room_id: &str,
        message: ServerMessage,
    ) -> anyhow::Result<()> {
        if let Some(connections) = self.connections.get(room_id) {
            for conn in connections.iter() {
                let _ = conn.sender.send(message.clone());
            }
        }
        Ok(())
    }

    pub async fn broadcast_to_room_except(
        &self,
        room_id: &str,
        exclude_player: PlayerId,
        message: ServerMessage,
    ) -> anyhow::Result<()> {
        if let Some(connections) = self.connections.get(room_id) {
            for conn in connections.iter() {
                if conn.player_id != exclude_player {
                    let _ = conn.sender.send(message.clone());
                }
            }
        }
        Ok(())
    }
    
    
    
    
}