use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

pub type PlayerId = u8;
pub type GridCell = u8;
pub type CompactGrid = Vec<u64>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoomResponse {
    pub room_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub room_id: String,
    pub canvas_size: (u32, u32),
    pub scores: (u32, u32),
    pub players: [Option<PlayerState>; 2],
    pub game_status: GameStatus,
    pub powerups_available: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    pub id: PlayerId,
    pub name: String,
    pub score: u32,
    pub powerup_cooldowns: HashMap<String, u32>,
    pub joined_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameStatus {
    WaitingForPlayers,
    Active,
    Ended { winner: Option<PlayerId>, reason: EndReason },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EndReason {
    PlayerSurrender,
    Timeout,
    // MaxSizeReached,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ClientMessage {
    JoinRoom { player_name: String },
    ClickBatch { 
        coordinates: Vec<(u32, u32)>, 
        timestamp: u64,
        sequence_id: u32 
    },
    ActivatePowerup { powerup_id: String },
    AcceptDefeat,
    Heartbeat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerMessage {
    RoomJoined { 
        room_id: String, 
        player_id: PlayerId, 
        initial_state: GameState,
        grid: Vec<u8>,
    },
    CanvasDelta {
        changes: Vec<GridChange>,
        new_scores: (u32, u32),
        canvas_size: (u32, u32),
        sequence_id: u32,
    },
    PowerupUpdate {
        available: Vec<String>,
        cooldowns: HashMap<String, u32>,
    },
    GameEnded { 
        winner: Option<PlayerId>, 
        reason: EndReason 
    },
    Error { message: String },
    GameStarted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridChange {
    pub x: u32,
    pub y: u32,
    pub old_owner: Option<PlayerId>,
    pub new_owner: PlayerId,
}

#[derive(Debug, Clone)]
pub struct ActiveRoom {
    pub room_id: String,
    pub canvas: Canvas,
    pub players: [Option<PlayerState>; 2],
    pub powerup_manager: PowerupManager,
    pub game_status: GameStatus,
    pub last_update: std::time::SystemTime,
    pub growth_tracker: CanvasGrowth,
}

#[derive(Debug, Clone)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub grid: CompactGrid,
}

#[derive(Debug, Clone)]
pub struct PowerupManager {
    pub available_powerups: Vec<String>,
    pub cooldowns: HashMap<String, HashMap<PlayerId, Instant>>,
}

#[derive(Debug, Clone)]
pub struct CanvasGrowth {
    pub last_expansion: Instant,
    pub current_size: (u32, u32),
    pub expansion_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PowerupResult {
    Success,
    OnCooldown,
    InvalidTarget,
    GameNotActive,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        let total_cells = (width * height) as usize;
        let u64_count = (total_cells + 63) / 64; // Round up for bit packing
        
        Self {
            width,
            height,
            grid: vec![0u64; u64_count],
        }
    }

    pub fn get_cell(&self, x: u32, y: u32) -> Option<PlayerId> {
        if x >= self.width || y >= self.height {
            return None;
        }
        
        let index = (y * self.width + x) as usize;
        let u64_index = index / 64;
        let bit_index = index % 64;
        
        if u64_index >= self.grid.len() {
            return None;
        }
        
        let bit = (self.grid[u64_index] >> bit_index) & 1;
        Some(bit as PlayerId)
    }

    pub fn set_cell(&mut self, x: u32, y: u32, player_id: PlayerId) -> bool {
        if x >= self.width || y >= self.height || player_id > 1 {
            return false;
        }
        
        let index = (y * self.width + x) as usize;
        let u64_index = index / 64;
        let bit_index = index % 64;
        
        if u64_index >= self.grid.len() {
            return false;
        }
        
        if player_id == 1 {
            self.grid[u64_index] |= 1u64 << bit_index;
        } else {
            self.grid[u64_index] &= !(1u64 << bit_index);
        }
        
        true
    }

    pub fn get_scores(&self) -> (u32, u32) {
        let mut score_0 = 0u32;
        let mut score_1 = 0u32;
        
        for y in 0..self.height {
            for x in 0..self.width {
                match self.get_cell(x, y) {
                    Some(0) => score_0 += 1,
                    Some(1) => score_1 += 1,
                    _ => {}
                }
            }
        }
        
        (score_0, score_1)
    }

    pub fn get_dominance_ratio(&self) -> f32 {
        let (score_0, score_1) = self.get_scores();
        let total = score_0 + score_1;
        
        if total == 0 {
            return 0.0;
        }
        
        let max_score = score_0.max(score_1);
        max_score as f32 / total as f32
    }
}

impl PowerupManager {
    pub fn new() -> Self {
        Self {
            available_powerups: vec![
                "grid_steal".to_string(),
                "canvas_freeze".to_string(),
                "double_click".to_string(),
                "territory_bomb".to_string(),
            ],
            cooldowns: HashMap::new(),
        }
    }

    pub fn can_activate(&self, powerup_id: &str, player_id: PlayerId) -> bool {
        if let Some(player_cooldowns) = self.cooldowns.get(powerup_id) {
            if let Some(last_used) = player_cooldowns.get(&player_id) {
                let cooldown_duration = match powerup_id {
                    "grid_steal" => std::time::Duration::from_secs(30),
                    "canvas_freeze" => std::time::Duration::from_secs(45),
                    "double_click" => std::time::Duration::from_secs(20),
                    "territory_bomb" => std::time::Duration::from_secs(60),
                    _ => std::time::Duration::from_secs(30),
                };
                
                return last_used.elapsed() >= cooldown_duration;
            }
        }
        true
    }
}

impl Default for PowerupManager {
    fn default() -> Self {
        Self::new()
    }
}