use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub max_concurrent_rooms: usize,
    pub room_cleanup_interval_secs: u64,
    pub canvas_config: CanvasConfig,
}

#[derive(Debug, Clone)]
pub struct CanvasConfig {
    pub initial_size: (u32, u32),
    pub growth_trigger: f32,
    pub growth_factor: f32,
    pub max_size: (u32, u32),
    pub cooldown_ms: u64,
}

impl Config {
    pub fn new() -> Self {
        let selff = Self {
            port: env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .unwrap_or(8080),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:Database.db".to_string()), // Simple filename in current directory
            max_concurrent_rooms: env::var("MAX_CONCURRENT_ROOMS")
                .unwrap_or_else(|_| "1000".to_string())
                .parse()
                .unwrap_or(1000),
            room_cleanup_interval_secs: env::var("ROOM_CLEANUP_INTERVAL")
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .unwrap_or(300),
            canvas_config: CanvasConfig {
                initial_size: (16, 16),
                growth_trigger: 0.65,
                growth_factor: 1.25,
                max_size: (512, 512),
                cooldown_ms: 5000,
            },
        };
        println!("Using database: {}", selff.database_url);
        return selff;
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}