/// Manages the state and lifecycle of the agentic loop for Qwen models
pub struct AgenticLoop {
    active: bool,
    turns: usize,
    max_turns: usize,
}

impl AgenticLoop {
    pub fn new(max_turns: usize) -> Self {
        Self {
            active: false,
            turns: 0,
            max_turns,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn start(&mut self) {
        eprintln!("[AGENTIC LOOP] Starting for Qwen model");
        self.active = true;
        self.turns = 0;
    }

    pub fn stop(&mut self, reason: &str) {
        eprintln!(
            "[AGENTIC LOOP] Stopping: {} (after {} turns)",
            reason, self.turns
        );
        self.active = false;
        self.turns = 0;
    }

    pub fn increment_turn(&mut self) {
        self.turns += 1;
        eprintln!("[AGENTIC LOOP] Turn {} for Qwen model", self.turns);

        if self.turns > self.max_turns {
            self.stop(&format!("reached max turns ({})", self.max_turns));
        }
    }
}
