use anyhow::Result;
use std::time::Instant;
use tracing::info;

use crate::types::*;
use super::ActiveRoom;

pub struct PowerupSystem {
    powerups: std::collections::HashMap<String, Box<dyn PowerupTrait + Send + Sync>>,
}

pub trait PowerupTrait {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn cooldown_seconds(&self) -> u32;
    fn can_activate(&self, player: &PlayerState, room: &ActiveRoom) -> bool;
    fn execute(&self, player_id: PlayerId, room: &mut ActiveRoom) -> Result<PowerupResult>;
}

impl PowerupSystem {
    pub fn new() -> Self {
        let mut powerups: std::collections::HashMap<String, Box<dyn PowerupTrait + Send + Sync>> = std::collections::HashMap::new();
        
        powerups.insert("grid_steal".to_string(), Box::new(GridStealPowerup));
        powerups.insert("canvas_freeze".to_string(), Box::new(CanvasFreezePowerup));
        powerups.insert("double_click".to_string(), Box::new(DoubleClickPowerup));
        powerups.insert("territory_bomb".to_string(), Box::new(TerritoryBombPowerup));
        
        Self { powerups }
    }

    pub async fn execute_powerup(
        &self,
        powerup_id: &str,
        player_id: PlayerId,
        room: &mut ActiveRoom,
    ) -> Result<PowerupResult> {
        let powerup = self.powerups.get(powerup_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown powerup: {}", powerup_id))?;

        let player = room.players[player_id as usize].as_ref()
            .ok_or_else(|| anyhow::anyhow!("Player not found"))?;

        if !powerup.can_activate(player, room) {
            return Ok(PowerupResult::OnCooldown);
        }

        let result = powerup.execute(player_id, room)?;

        // Update cooldown
        let cooldown_map = room.powerup_manager.cooldowns
            .entry(powerup_id.to_string())
            .or_insert_with(std::collections::HashMap::new);
        cooldown_map.insert(player_id, Instant::now());

        info!("Player {} activated powerup {} in room {}", 
              player_id, powerup_id, room.room_id);

        Ok(result)
    }
}

// GridSteal Powerup - Transfers 10% of opponent's grids
struct GridStealPowerup;

impl PowerupTrait for GridStealPowerup {
    fn id(&self) -> &'static str { "grid_steal" }
    fn name(&self) -> &'static str { "Grid Steal" }
    fn cooldown_seconds(&self) -> u32 { 30 }
    
    fn can_activate(&self, _player: &PlayerState, room: &ActiveRoom) -> bool {
        matches!(room.game_status, GameStatus::Active)
    }
    
    fn execute(&self, player_id: PlayerId, room: &mut ActiveRoom) -> Result<PowerupResult> {
        let opponent_id = if player_id == 0 { 1 } else { 0 };
        let mut opponent_cells = Vec::new();
        
        // Collect opponent's cells
        for y in 0..room.canvas.height {
            for x in 0..room.canvas.width {
                if let Some(owner) = room.canvas.get_cell(x, y) {
                    if owner == opponent_id {
                        opponent_cells.push((x, y));
                    }
                }
            }
        }
        
        // Steal 10% of opponent's cells
        let steal_count = (opponent_cells.len() as f32 * 0.1).ceil() as usize;
        let cells_to_steal = opponent_cells.into_iter().take(steal_count);
        
        for (x, y) in cells_to_steal {
            room.canvas.set_cell(x, y, player_id);
        }
        
        // Update scores
        let (score_0, score_1) = room.canvas.get_scores();
        if let Some(ref mut player) = room.players[0] {
            player.score = score_0;
        }
        if let Some(ref mut player) = room.players[1] {
            player.score = score_1;
        }
        
        Ok(PowerupResult::Success)
    }
}

// CanvasFreeze Powerup - Prevents opponent from clicking for 3 seconds
struct CanvasFreezePowerup;

impl PowerupTrait for CanvasFreezePowerup {
    fn id(&self) -> &'static str { "canvas_freeze" }
    fn name(&self) -> &'static str { "Canvas Freeze" }
    fn cooldown_seconds(&self) -> u32 { 45 }
    
    fn can_activate(&self, _player: &PlayerState, room: &ActiveRoom) -> bool {
        matches!(room.game_status, GameStatus::Active)
    }
    
    fn execute(&self, _player_id: PlayerId, _room: &mut ActiveRoom) -> Result<PowerupResult> {
        // Note: Freeze effect would be handled by the WebSocket layer
        // This powerup doesn't modify the canvas directly
        Ok(PowerupResult::Success)
    }
}

// DoubleClick Powerup - Next 10 clicks count as 2
struct DoubleClickPowerup;

impl PowerupTrait for DoubleClickPowerup {
    fn id(&self) -> &'static str { "double_click" }
    fn name(&self) -> &'static str { "Double Click" }
    fn cooldown_seconds(&self) -> u32 { 20 }
    
    fn can_activate(&self, _player: &PlayerState, room: &ActiveRoom) -> bool {
        matches!(room.game_status, GameStatus::Active)
    }
    
    fn execute(&self, _player_id: PlayerId, _room: &mut ActiveRoom) -> Result<PowerupResult> {
        // Note: Double click effect would be handled by the click processing logic
        Ok(PowerupResult::Success)
    }
}

// TerritoryBomb Powerup - Clears a 3x3 area
struct TerritoryBombPowerup;

impl PowerupTrait for TerritoryBombPowerup {
    fn id(&self) -> &'static str { "territory_bomb" }
    fn name(&self) -> &'static str { "Territory Bomb" }
    fn cooldown_seconds(&self) -> u32 { 60 }
    
    fn can_activate(&self, _player: &PlayerState, room: &ActiveRoom) -> bool {
        matches!(room.game_status, GameStatus::Active)
    }
    
    fn execute(&self, player_id: PlayerId, room: &mut ActiveRoom) -> Result<PowerupResult> {
        let opponent_id = if player_id == 0 { 1 } else { 0 };
        
        // Find a 3x3 area with opponent cells to bomb
        let mut best_target = None;
        let mut max_opponent_cells = 0;
        
        for center_y in 1..(room.canvas.height - 1) {
            for center_x in 1..(room.canvas.width - 1) {
                let mut opponent_count = 0;
                
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let x = (center_x as i32 + dx) as u32;
                        let y = (center_y as i32 + dy) as u32;
                        
                        if let Some(owner) = room.canvas.get_cell(x, y) {
                            if owner == opponent_id {
                                opponent_count += 1;
                            }
                        }
                    }
                }
                
                if opponent_count > max_opponent_cells {
                    max_opponent_cells = opponent_count;
                    best_target = Some((center_x, center_y));
                }
            }
        }
        
        // Clear the 3x3 area around the best target
        if let Some((center_x, center_y)) = best_target {
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let x = (center_x as i32 + dx) as u32;
                    let y = (center_y as i32 + dy) as u32;
                    
                    if x < room.canvas.width && y < room.canvas.height {
                        room.canvas.set_cell(x, y, player_id);
                    }
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
        
        Ok(PowerupResult::Success)
    }
}