-- Users table (for stats and optional auth)
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    email TEXT UNIQUE,
    password_hash TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Rooms (games)
CREATE TABLE rooms (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_code TEXT UNIQUE NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at DATETIME,
    ended_at DATETIME,
    status TEXT NOT NULL,
    settings_json TEXT,
    winner_user_id INTEGER,
    FOREIGN KEY (winner_user_id) REFERENCES users(id)
);

-- Room players (who played in which room)
CREATE TABLE room_players (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id INTEGER NOT NULL,
    user_id INTEGER,
    player_name TEXT NOT NULL,
    player_index INTEGER NOT NULL,
    joined_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    left_at DATETIME,
    is_guest BOOLEAN NOT NULL DEFAULT 0,
    FOREIGN KEY (room_id) REFERENCES rooms(id),
    FOREIGN KEY (user_id) REFERENCES users(id)
);

-- Moves (every tap/click/action)
CREATE TABLE moves (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    move_type TEXT NOT NULL,
    move_data_json TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (room_id) REFERENCES rooms(id),
    FOREIGN KEY (player_id) REFERENCES room_players(id)
);

-- Powerup usage
CREATE TABLE powerup_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    powerup_type TEXT NOT NULL,
    used_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    result TEXT,
    FOREIGN KEY (room_id) REFERENCES rooms(id),
    FOREIGN KEY (player_id) REFERENCES room_players(id)
);

-- Custom abilities (created/selected for a room)
CREATE TABLE custom_abilities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    ability_name TEXT NOT NULL,
    ability_data_json TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (room_id) REFERENCES rooms(id),
    FOREIGN KEY (player_id) REFERENCES room_players(id)
);

-- Ability usage (every use of a custom ability)
CREATE TABLE ability_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    ability_id INTEGER NOT NULL,
    used_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    result TEXT,
    FOREIGN KEY (room_id) REFERENCES rooms(id),
    FOREIGN KEY (player_id) REFERENCES room_players(id),
    FOREIGN KEY (ability_id) REFERENCES custom_abilities(id)
);

-- Commentary log (all commentary events)
CREATE TABLE commentary_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    message TEXT NOT NULL,
    triggered_by_player_id INTEGER,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (room_id) REFERENCES rooms(id),
    FOREIGN KEY (triggered_by_player_id) REFERENCES room_players(id)
);

-- Room stats (aggregated, for fast queries)
CREATE TABLE room_stats (
    room_id INTEGER PRIMARY KEY,
    total_moves INTEGER,
    total_powerups INTEGER,
    total_abilities INTEGER,
    duration_seconds INTEGER,
    winner_player_id INTEGER,
    FOREIGN KEY (room_id) REFERENCES rooms(id),
    FOREIGN KEY (winner_player_id) REFERENCES room_players(id)
); 