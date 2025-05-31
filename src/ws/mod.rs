use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde_json;
use std::sync::Arc;
// use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;

use tracing::{error, info, warn};

use crate::{types::*, AppState};

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| websocket_connection(socket, room_id, state))
}

async fn websocket_connection(socket: WebSocket, room_id: String, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ServerMessage>();
    
    let room_id_clone = room_id.clone();
    let state_clone = state.clone();
    
    // Handle outgoing messages
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json_msg = match serde_json::to_string(&msg) {
                Ok(json) => json,
                Err(e) => {
                    error!("Failed to serialize message: {}", e);
                    continue;
                }
            };
            
            if sender.send(Message::Text(json_msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    let mut player_id: Option<PlayerId> = None;
    let mut sequence_counter = 0u32;
    
    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                warn!("WebSocket error: {}", e);
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                let client_msg: ClientMessage = match serde_json::from_str(&text) {
                    Ok(msg) => msg,
                    Err(e) => {
                        error!("Failed to parse client message: {}", e);
                        let error_msg = ServerMessage::Error {
                            message: "Invalid message format".to_string(),
                        };
                        if tx.send(error_msg).is_err() {
                            break;
                        }
                        continue;
                    }
                };

                match handle_client_message(
                    client_msg,
                    &room_id,
                    &mut player_id,
                    &mut sequence_counter,
                    &state,
                    &tx,
                ).await {
                    Ok(()) => {}
                    Err(e) => {
                        error!("Error handling message: {}", e);
                        let error_msg = ServerMessage::Error {
                            message: e.to_string(),
                        };
                        if tx.send(error_msg).is_err() {
                            break;
                        }
                    }
                }
            }
            Message::Binary(_) => {
                warn!("Received unexpected binary message");
            }
            Message::Close(_) => {
                info!("WebSocket connection closed for room: {}", room_id);
                break;
            }
            _ => {}
        }
    }

    send_task.abort();
    info!("WebSocket connection ended for room: {}", room_id);
}

async fn handle_client_message(
    message: ClientMessage,
    room_id: &str,
    player_id: &mut Option<PlayerId>,
    sequence_counter: &mut u32,
    state: &AppState,
    tx: &UnboundedSender<ServerMessage>,
) -> anyhow::Result<()> {
    match message {
        // ClientMessage::JoinRoom { player_name } => {
        //     let (pid, game_state) = state.game_manager.join_room(room_id, player_name).await?;
        //     *player_id = Some(pid);
        
        //     // Register this connection for broadcasting
        //     state.game_manager.register_connection(room_id.to_string(), pid, tx.clone());
        
        //     // ✅ Get the real canvas from the active room
        //     let canvas = {
        //         let room_lock = state.game_manager.rooms.get(room_id)
        //             .ok_or_else(|| anyhow::anyhow!("Room not found"))?;
        //         let room = room_lock.read().await;
        //         room.canvas.clone()
        //     };
            
        //     let grid = state.game_manager.get_flattened_grid(&canvas);
        
        //     // ✅ Include the grid in the message
        //     let response = ServerMessage::RoomJoined {
        //         room_id: room_id.to_string(),
        //         player_id: pid,
        //         initial_state: game_state,
        //         grid,
        //     };
        
        //     tx.send(response).map_err(|e| anyhow::anyhow!("Send failed: {}", e))?;
            
        //     info!("Player {} joined room {}", pid, room_id);
        // }
        // ClientMessage::JoinRoom { player_name } => {
        //     let (pid, game_state) = state.game_manager.join_room(room_id, player_name).await?;
        //     *player_id = Some(pid);
        
        //     // Register this player's connection
        //     state.game_manager.register_connection(room_id.to_string(), pid, tx.clone());
        
        //     // Get the current canvas to flatten
        //     let room_lock = state.game_manager.rooms.get(room_id).ok_or_else(|| anyhow::anyhow!("Room not found"))?;
        //     let room = room_lock.read().await;
        //     let grid = state.game_manager.get_flattened_grid(&room.canvas);
        
        //     // Send initial state and grid to this player
        //     let response = ServerMessage::RoomJoined {
        //         room_id: room_id.to_string(),
        //         player_id: pid,
        //         initial_state: game_state,
        //         grid,
        //     };
        //     tx.send(response.clone())?;
        //     state.game_manager.broadcast_to_room_except(room_id,pid, response).await?;
        //     info!("Player {} joined room {}", pid, room_id);
        // }
        
        // In ws.rs - Fix the JoinRoom message handling

ClientMessage::JoinRoom { player_name } => {
    let (pid, game_state) = state.game_manager.join_room(room_id, player_name).await?;
    *player_id = Some(pid);

    // Register this player's connection
    state.game_manager.register_connection(room_id.to_string(), pid, tx.clone());

    // Get the current canvas to flatten
    let room_lock = state.game_manager.rooms.get(room_id)
        .ok_or_else(|| anyhow::anyhow!("Room not found"))?;
    let room = room_lock.read().await;
    let grid = state.game_manager.get_flattened_grid(&room.canvas);

    // Send initial state and grid to this player
    let response = ServerMessage::RoomJoined {
        room_id: room_id.to_string(),
        player_id: pid,
        initial_state: game_state,
        grid,
    };
    tx.send(response)?;
    
    // If this is the second player, start the game
    if room.players[0].is_some() && room.players[1].is_some() {
        drop(room); // Release the read lock
        let start_msg = ServerMessage::GameStarted;
        state.game_manager.broadcast_to_room(room_id, start_msg).await?;
    }
    
    info!("Player {} joined room {}", pid, room_id);
}

// Fix ClickBatch handling - remove the broadcast_to_room_except call
ClientMessage::ClickBatch { coordinates, timestamp: _, sequence_id } => {
    let pid = player_id.ok_or_else(|| anyhow::anyhow!("Player not joined"))?;

    let changes = state.game_manager.process_click_batch(
        room_id,
        pid,
        coordinates,
        sequence_id,
    ).await?;

    if !changes.is_empty() {
        *sequence_counter += 1;

        // Get updated room state for scores and canvas size
        let room_state = state.game_manager.get_room_state(room_id).await
            .ok_or_else(|| anyhow::anyhow!("Room not found"))?;

        let response = ServerMessage::CanvasDelta {
            changes,
            new_scores: room_state.scores,
            canvas_size: room_state.canvas_size,
            sequence_id: *sequence_counter,
        };

        // Broadcast to everyone in the room
        state.game_manager.broadcast_to_room(room_id, response).await?;
    }
}

        // ClientMessage::ClickBatch { coordinates, timestamp: _, sequence_id } => {
        //     let pid = player_id.ok_or_else(|| anyhow::anyhow!("Player not joined"))?;
        
        //     let changes = state.game_manager.process_click_batch(
        //         room_id,
        //         pid,
        //         coordinates,
        //         sequence_id,
        //     ).await?;
        
        //     if !changes.is_empty() {
        //         *sequence_counter += 1;
        
        //         // Get updated room state for scores and canvas size
        //         let room_state = state.game_manager.get_room_state(room_id).await
        //             .ok_or_else(|| anyhow::anyhow!("Room not found"))?;
        
        //         let response = ServerMessage::CanvasDelta {
        //             changes,
        //             new_scores: room_state.scores,
        //             canvas_size: room_state.canvas_size,
        //             sequence_id: *sequence_counter,
        //         };
        
        //         // ✅ Broadcast to everyone, not just this player
        //         state.game_manager.broadcast_to_room(room_id, response).await?;
        //     }
        // }
        

        ClientMessage::ActivatePowerup { powerup_id } => {
            let pid = player_id.ok_or_else(|| anyhow::anyhow!("Player not joined"))?;
            
            let result = state.game_manager.activate_powerup(room_id, pid, &powerup_id).await?;
            
            match result {
                PowerupResult::Success => {
                    // Get updated powerup cooldowns
                    if let Some(room_state) = state.game_manager.get_room_state(room_id).await {
                        let player_state = &room_state.players[pid as usize];
                        if let Some(player) = player_state {
                            let response = ServerMessage::PowerupUpdate {
                                available: room_state.powerups_available,
                                cooldowns: player.powerup_cooldowns.clone(),
                            };
                            state.game_manager.broadcast_to_room(room_id, response).await?;
                        }
                    }
                    
                    info!("Player {} activated powerup {} in room {}", pid, powerup_id, room_id);
                }
                _ => {
                    let error_msg = ServerMessage::Error {
                        message: format!("Powerup activation failed: {:?}", result),
                    };
                    tx.send(error_msg)?;
                }
            }
        }

        ClientMessage::AcceptDefeat => {
            let pid = player_id.ok_or_else(|| anyhow::anyhow!("Player not joined"))?;
            
            state.game_manager.accept_defeat(room_id, pid).await?;
            
            let winner = if pid == 0 { Some(1) } else { Some(0) };
            let response = ServerMessage::GameEnded {
                winner,
                reason: EndReason::PlayerSurrender,
            };
            
            state.game_manager.broadcast_to_room(room_id, response).await?;

            info!("Player {} surrendered in room {}", pid, room_id);
        }

        ClientMessage::Heartbeat => {
            // Heartbeat received, connection is alive
        }
    }

    Ok(())
}