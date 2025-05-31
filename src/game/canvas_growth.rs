use std::time::{Duration, Instant};

use crate::types::*;

pub struct CanvasGrowthSystem {
    growth_trigger: f32,
    growth_factor: f32,
    max_size: (u32, u32),
    cooldown_ms: u64,
}

impl CanvasGrowthSystem {
    pub fn new() -> Self {
        Self {
            growth_trigger: 0.85,   // 65% dominance triggers growth
            growth_factor: 1.1,    // 25% size increase per expansion
            max_size: (512, 512),   // Maximum canvas size
            cooldown_ms: 5000,      // 5 second cooldown between expansions
        }
    }

    pub fn should_expand(&self, canvas: &Canvas, growth_tracker: &CanvasGrowth) -> bool {
        // Check if we've reached max size
        if growth_tracker.current_size.0 >= self.max_size.0 || 
           growth_tracker.current_size.1 >= self.max_size.1 {
            return false;
        }

        // Check cooldown
        if growth_tracker.last_expansion.elapsed().as_millis() < self.cooldown_ms as u128 {
            return false;
        }

        // Check dominance ratio
        let dominance_ratio = canvas.get_dominance_ratio();
        dominance_ratio > self.growth_trigger
    }

    pub fn calculate_new_size(&self, growth_tracker: &CanvasGrowth) -> (u32, u32) {
        let current_size = growth_tracker.current_size;
        
        match self.get_growth_pattern(growth_tracker.expansion_count) {
            GrowthPattern::Linear => {
                let increment = 4; // Add 4 cells per side
                (
                    (current_size.0 + increment).min(self.max_size.0),
                    (current_size.1 + increment).min(self.max_size.1),
                )
            },
            GrowthPattern::Exponential => {
                let new_width = ((current_size.0 as f32 * self.growth_factor) as u32).min(self.max_size.0);
                let new_height = ((current_size.1 as f32 * self.growth_factor) as u32).min(self.max_size.1);
                (new_width, new_height)
            },
            GrowthPattern::Fibonacci => {
                let fib_increment = self.fibonacci_sequence(growth_tracker.expansion_count as usize + 3);
                (
                    (current_size.0 + fib_increment).min(self.max_size.0),
                    (current_size.1 + fib_increment).min(self.max_size.1),
                )
            },
            GrowthPattern::Adaptive => {
                // Adaptive growth based on how quickly dominance was achieved
                let time_since_last = growth_tracker.last_expansion.elapsed().as_secs();
                let growth_multiplier = if time_since_last < 30 {
                    1.5 // Fast dominance = bigger growth
                } else if time_since_last < 60 {
                    1.3
                } else {
                    1.2 // Slow dominance = smaller growth
                };
                
                let new_width = ((current_size.0 as f32 * growth_multiplier) as u32).min(self.max_size.0);
                let new_height = ((current_size.1 as f32 * growth_multiplier) as u32).min(self.max_size.1);
                (new_width, new_height)
            }
        }
    }

    fn get_growth_pattern(&self, expansion_count: u32) -> GrowthPattern {
        // Switch patterns as game progresses
        match expansion_count {
            0..=2 => GrowthPattern::Linear,        // Early game: steady growth
            3..=5 => GrowthPattern::Exponential,   // Mid game: accelerating
            6..=8 => GrowthPattern::Fibonacci,     // Late game: mathematical
            _ => GrowthPattern::Adaptive,          // End game: dynamic
        }
    }

    fn fibonacci_sequence(&self, n: usize) -> u32 {
        match n {
            0 => 0,
            1 => 1,
            _ => {
                let mut a = 0u32;
                let mut b = 1u32;
                for _ in 2..=n {
                    let temp = a + b;
                    a = b;
                    b = temp;
                }
                b.min(50) // Cap fibonacci growth to reasonable increments
            }
        }
    }
}

#[derive(Debug, Clone)]
enum GrowthPattern {
    Linear,      // Consistent increments
    Exponential, // Accelerating expansion  
    Fibonacci,   // Mathematical progression
    Adaptive,    // Based on game dynamics
}