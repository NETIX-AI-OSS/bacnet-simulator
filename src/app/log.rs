use std::collections::VecDeque;
use std::sync::Mutex;

const LOG_CAPACITY: usize = 50;

#[derive(Debug, Default)]
pub struct AppLog {
    lines: Mutex<VecDeque<String>>,
}

impl AppLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, line: impl Into<String>) {
        if let Ok(mut guard) = self.lines.lock() {
            guard.push_back(line.into());
            while guard.len() > LOG_CAPACITY {
                guard.pop_front();
            }
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }
}
