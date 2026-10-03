//! The scheduler's ready queue: a binary max-heap keyed by critical-path
//! length, ties broken FIFO by insertion order. O(log n) push / pop.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

#[derive(Debug)]
struct Item<T> {
    priority: u32,
    seq: Reverse<u64>,
    value: T,
}

impl<T> PartialEq for Item<T> {
    fn eq(&self, other: &Self) -> bool {
        (self.priority, self.seq) == (other.priority, other.seq)
    }
}

impl<T> Eq for Item<T> {}

impl<T> PartialOrd for Item<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Item<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.priority, self.seq).cmp(&(other.priority, other.seq))
    }
}

/// Highest priority first; equal priorities in insertion order.
#[derive(Debug)]
pub struct ReadyQueue<T> {
    heap: BinaryHeap<Item<T>>,
    next_seq: u64,
}

impl<T> Default for ReadyQueue<T> {
    fn default() -> Self {
        ReadyQueue {
            heap: BinaryHeap::new(),
            next_seq: 0,
        }
    }
}

impl<T> ReadyQueue<T> {
    /// Enqueues `value` with `priority`.
    pub fn push(&mut self, priority: u32, value: T) {
        self.heap.push(Item {
            priority,
            seq: Reverse(self.next_seq),
            value,
        });
        self.next_seq += 1;
    }

    /// Removes the highest priority value.
    pub fn pop(&mut self) -> Option<T> {
        self.heap.pop().map(|i| i.value)
    }

    /// Number of queued values.
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// True when nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_by_priority_then_fifo() {
        let mut q = ReadyQueue::default();
        q.push(1, "low");
        q.push(5, "high-1");
        q.push(3, "mid");
        q.push(5, "high-2");
        assert_eq!(q.len(), 4);
        let order: Vec<_> = std::iter::from_fn(|| q.pop()).collect();
        assert_eq!(order, vec!["high-1", "high-2", "mid", "low"]);
        assert!(q.is_empty());
    }
}
