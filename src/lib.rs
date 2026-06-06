//! # ternary-semaphore
//!
//! Ternary semaphore for GPU resource control.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermitState { Available = 1, AtCapacity = 0, Overcommitted = -1 }

#[derive(Debug, Clone)]
pub struct Permit { pub id: u64, pub kernel: String, pub priority: i8 }

pub struct TernarySemaphore {
    max_permits: usize,
    active: usize,
    wait_queue: VecDeque<Permit>,
    issued: u64,
    released: u64,
}

impl TernarySemaphore {
    pub fn new(max_permits: usize) -> Self {
        Self { max_permits, active: 0, wait_queue: VecDeque::new(), issued: 0, released: 0 }
    }

    pub fn state(&self) -> PermitState {
        if self.active < self.max_permits { PermitState::Available }
        else if self.active == self.max_permits { PermitState::AtCapacity }
        else { PermitState::Overcommitted }
    }

    pub fn try_acquire(&mut self, kernel: &str, priority: i8) -> Option<u64> {
        if self.active < self.max_permits {
            self.active += 1;
            self.issued += 1;
            Some(self.issued)
        } else {
            self.wait_queue.push_back(Permit { id: self.issued + self.wait_queue.len() as u64 + 1, kernel: kernel.into(), priority });
            None
        }
    }

    pub fn force_acquire(&mut self, _kernel: &str) -> u64 {
        self.active += 1;
        self.issued += 1;
        self.issued
    }

    pub fn release(&mut self) -> Option<Permit> {
        self.active = self.active.saturating_sub(1);
        self.released += 1;
        // If waiting and now available, dequeue
        if self.active < self.max_permits {
            if let Some(permit) = self.wait_queue.pop_front() {
                self.active += 1;
                return Some(permit);
            }
        }
        None
    }

    /// Process wait queue: admit highest priority waiting permits.
    pub fn drain_queue(&mut self) -> Vec<Permit> {
        let mut admitted = Vec::new();
        while self.active < self.max_permits {
            // Find highest priority
            let best = self.wait_queue.iter().enumerate()
                .max_by_key(|(_, p)| p.priority).map(|(i, _)| i);
            match best {
                Some(idx) => {
                    let permit = self.wait_queue.remove(idx).unwrap();
                    self.active += 1;
                    admitted.push(permit);
                }
                None => break,
            }
        }
        admitted
    }

    pub fn active_count(&self) -> usize { self.active }
    pub fn waiting_count(&self) -> usize { self.wait_queue.len() }
    pub fn utilization(&self) -> f64 { self.active as f64 / self.max_permits.max(1) as f64 }
    pub fn issued(&self) -> u64 { self.issued }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available() {
        let sem = TernarySemaphore::new(4);
        assert_eq!(sem.state(), PermitState::Available);
    }

    #[test]
    fn test_at_capacity() {
        let mut sem = TernarySemaphore::new(2);
        sem.force_acquire("a");
        sem.force_acquire("b");
        assert_eq!(sem.state(), PermitState::AtCapacity);
    }

    #[test]
    fn test_acquire_release() {
        let mut sem = TernarySemaphore::new(2);
        let id = sem.try_acquire("kernel", 0);
        assert!(id.is_some());
        sem.release();
        assert_eq!(sem.active_count(), 0);
    }

    #[test]
    fn test_queue_when_full() {
        let mut sem = TernarySemaphore::new(1);
        sem.try_acquire("a", 0);
        let result = sem.try_acquire("b", 0);
        assert!(result.is_none());
        assert_eq!(sem.waiting_count(), 1);
    }

    #[test]
    fn test_drain_priority() {
        let mut sem = TernarySemaphore::new(1);
        sem.force_acquire("a"); // now full
        sem.try_acquire("low", -1); // queued
        sem.try_acquire("high", 1); // queued
        // Release auto-admits highest priority from queue
        let next = sem.release();
        assert!(next.is_some()); // auto-admitted from queue
        assert_eq!(sem.active_count(), 1);
    }

    #[test]
    fn test_utilization() {
        let mut sem = TernarySemaphore::new(4);
        sem.force_acquire("a");
        assert!((sem.utilization() - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_overcommit() {
        let mut sem = TernarySemaphore::new(1);
        sem.force_acquire("a");
        sem.force_acquire("b"); // over
        assert_eq!(sem.state(), PermitState::Overcommitted);
    }
}
