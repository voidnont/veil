use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Browser -> renderer interaction message. 0.6 uses stable node ids so UI
/// events can be routed back to the site renderer without reparsing a page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RuntimeEvent {
    Click { node_id: usize, x: f32, y: f32, button: u8 },
    Input { node_id: usize, value: String },
    Change { node_id: usize, value: String },
    Submit { node_id: usize },
    Key { node_id: Option<usize>, key: String, code: String, pressed: bool, repeat: bool },
    Focus { node_id: usize },
    Blur { node_id: usize },
    Media { node_id: usize, kind: String, current_time: f64 },
    AnimationFrame { timestamp_ms: f64 },
    Timer { timer_id: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueuedRuntimeEvent {
    pub sequence: u64,
    pub event: RuntimeEvent,
}

/// Bounded deterministic queue owned by one site runtime. A production browser
/// eventually maps this to the renderer process' JS task/microtask queues.
#[derive(Debug)]
pub struct RuntimeEventLoop {
    next_sequence: u64,
    queue: VecDeque<QueuedRuntimeEvent>,
    timers: HashMap<u64, TimerEntry>,
    next_timer: u64,
    started: Instant,
}

#[derive(Debug, Clone)]
struct TimerEntry {
    deadline: Instant,
    interval: Option<Duration>,
}

impl Default for RuntimeEventLoop {
    fn default() -> Self {
        Self {
            next_sequence: 1,
            queue: VecDeque::new(),
            timers: HashMap::new(),
            next_timer: 1,
            started: Instant::now(),
        }
    }
}

impl RuntimeEventLoop {
    pub const MAX_PENDING_EVENTS: usize = 4096;
    pub const MAX_TIMERS: usize = 512;

    pub fn push(&mut self, event: RuntimeEvent) -> Result<u64, String> {
        if self.queue.len() >= Self::MAX_PENDING_EVENTS {
            return Err("site runtime event queue is full".into());
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.queue.push_back(QueuedRuntimeEvent { sequence, event });
        Ok(sequence)
    }

    pub fn pop(&mut self) -> Option<QueuedRuntimeEvent> { self.queue.pop_front() }
    pub fn len(&self) -> usize { self.queue.len() }
    pub fn is_empty(&self) -> bool { self.queue.is_empty() }

    pub fn set_timeout(&mut self, delay: Duration, repeat: bool) -> Result<u64, String> {
        if self.timers.len() >= Self::MAX_TIMERS { return Err("site timer limit reached".into()); }
        let id = self.next_timer;
        self.next_timer = self.next_timer.saturating_add(1);
        self.timers.insert(id, TimerEntry {
            deadline: Instant::now() + delay.min(Duration::from_secs(24 * 60 * 60)),
            interval: repeat.then_some(delay.max(Duration::from_millis(4))),
        });
        Ok(id)
    }

    pub fn clear_timer(&mut self, id: u64) { self.timers.remove(&id); }

    pub fn pump_timers(&mut self, now: Instant) {
        let mut due = Vec::new();
        for (&id, entry) in &self.timers {
            if entry.deadline <= now { due.push(id); }
        }
        for id in due {
            if self.queue.len() < Self::MAX_PENDING_EVENTS {
                let _ = self.push(RuntimeEvent::Timer { timer_id: id });
            }
            if let Some(entry) = self.timers.get_mut(&id) {
                if let Some(interval) = entry.interval { entry.deadline = now + interval; }
                else { self.timers.remove(&id); }
            }
        }
    }

    pub fn animation_timestamp_ms(&self) -> f64 { self.started.elapsed().as_secs_f64() * 1000.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_event_order() {
        let mut loop_ = RuntimeEventLoop::default();
        loop_.push(RuntimeEvent::Focus { node_id: 1 }).unwrap();
        loop_.push(RuntimeEvent::Click { node_id: 1, x: 4.0, y: 7.0, button: 0 }).unwrap();
        assert_eq!(loop_.pop().unwrap().sequence, 1);
        assert_eq!(loop_.pop().unwrap().sequence, 2);
    }

    #[test]
    fn timers_are_bounded() {
        let mut loop_ = RuntimeEventLoop::default();
        let id = loop_.set_timeout(Duration::ZERO, false).unwrap();
        loop_.pump_timers(Instant::now() + Duration::from_millis(1));
        assert!(matches!(loop_.pop().map(|x| x.event), Some(RuntimeEvent::Timer { timer_id }) if timer_id == id));
    }
}
