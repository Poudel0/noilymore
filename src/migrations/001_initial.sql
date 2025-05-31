-- Game Results Table
CREATE TABLE IF NOT EXISTS game_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id TEXT NOT NULL,
    winner INTEGER, -- 0, 1, or NULL for tie
    duration_seconds INTEGER NOT NULL,
    final_score_0 INTEGER NOT NULL,
    final_score_1 INTEGER NOT NULL,
    final_width INTEGER NOT NULL,
    final_height INTEGER NOT NULL,
    total_clicks INTEGER NOT NULL,
    powerups_used INTEGER NOT NULL,
    end_reason TEXT NOT NULL, -- JSON serialized EndReason
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Player Statistics Table
CREATE TABLE IF NOT EXISTS player_stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    player_name TEXT UNIQUE NOT NULL,
    total_games INTEGER NOT NULL DEFAULT 0,
    games_won INTEGER NOT NULL DEFAULT 0,
    total_clicks INTEGER NOT NULL DEFAULT 0,
    favorite_powerup TEXT,
    average_game_duration REAL NOT NULL DEFAULT 0.0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Canvas Snapshots Table (for optional replay data)
CREATE TABLE IF NOT EXISTS canvas_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id TEXT NOT NULL,
    canvas_data BLOB NOT NULL, -- Compressed canvas state
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- System Metrics Table
CREATE TABLE IF NOT EXISTS system_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    metric_name TEXT NOT NULL,
    metric_value REAL NOT NULL,
    recorded_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_game_results_room_id ON game_results(room_id);
CREATE INDEX IF NOT EXISTS idx_game_results_created_at ON game_results(created_at);
CREATE INDEX IF NOT EXISTS idx_player_stats_name ON player_stats(player_name);
CREATE INDEX IF NOT EXISTS