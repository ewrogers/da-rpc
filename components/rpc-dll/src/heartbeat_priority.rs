use std::sync::{Mutex, MutexGuard};

pub(crate) const HEARTBEAT_OPCODE: u8 = 0x45;
pub(crate) const CAPACITY: usize = 8;
pub(crate) const DESCRIPTOR_WORDS: usize = 6;

pub(crate) type Descriptor = [usize; DESCRIPTOR_WORDS];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Entry {
    queue: usize,
    descriptor: Descriptor,
}

#[derive(Debug)]
struct PriorityQueue {
    entries: [Option<Entry>; CAPACITY],
    len: usize,
}

impl PriorityQueue {
    const fn new() -> Self {
        Self {
            entries: [None; CAPACITY],
            len: 0,
        }
    }

    fn push(&mut self, entry: Entry) -> bool {
        if self.len == CAPACITY {
            return false;
        }
        self.entries[self.len] = Some(entry);
        self.len += 1;
        true
    }

    fn remove(&mut self, index: usize) -> Entry {
        let entry = self.entries[index].take().expect("priority entry exists");
        for position in index..self.len - 1 {
            self.entries[position] = self.entries[position + 1].take();
        }
        self.len -= 1;
        entry
    }

    fn pop_for(&mut self, queue: usize) -> Option<Descriptor> {
        let index = self.entries[..self.len]
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.queue == queue))?;
        Some(self.remove(index).descriptor)
    }
}

static PRIORITY_QUEUE: Mutex<PriorityQueue> = Mutex::new(PriorityQueue::new());

fn lock_queue() -> MutexGuard<'static, PriorityQueue> {
    PRIORITY_QUEUE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn is_heartbeat(buffer: &[u8]) -> bool {
    buffer.first() == Some(&HEARTBEAT_OPCODE)
}

pub(crate) fn push_and_signal(
    queue: usize,
    descriptor: Descriptor,
    signal: impl FnOnce() -> bool,
) -> bool {
    let mut priority = lock_queue();
    if !priority.push(Entry { queue, descriptor }) {
        return false;
    }
    if signal() {
        true
    } else {
        let last = priority.len - 1;
        priority.remove(last);
        false
    }
}

pub(crate) fn pop_for(queue: usize) -> Option<Descriptor> {
    lock_queue().pop_for(queue)
}

pub(crate) fn is_empty() -> bool {
    lock_queue().len == 0
}

#[cfg(test)]
mod tests {
    use super::{
        CAPACITY, DESCRIPTOR_WORDS, Entry, PriorityQueue, is_empty, is_heartbeat, pop_for,
        push_and_signal,
    };

    fn descriptor(value: usize) -> [usize; DESCRIPTOR_WORDS] {
        [value; DESCRIPTOR_WORDS]
    }

    #[test]
    fn recognizes_only_heartbeat_packets() {
        assert!(is_heartbeat(&[0x45, 0, 0, 0]));
        assert!(!is_heartbeat(&[0x44, 0, 0, 0]));
        assert!(!is_heartbeat(&[]));
    }

    #[test]
    fn heartbeat_queue_is_bounded_and_preserves_order() {
        let mut queue = PriorityQueue::new();
        for value in 0..CAPACITY {
            assert!(queue.push(Entry {
                queue: 7,
                descriptor: descriptor(value)
            }));
        }
        assert!(!queue.push(Entry {
            queue: 7,
            descriptor: descriptor(CAPACITY)
        }));
        for value in 0..CAPACITY {
            assert_eq!(queue.pop_for(7), Some(descriptor(value)));
        }
        assert_eq!(queue.pop_for(7), None);
    }

    #[test]
    fn independent_transport_queues_do_not_block_each_other() {
        let mut queue = PriorityQueue::new();
        assert!(queue.push(Entry {
            queue: 1,
            descriptor: descriptor(10)
        }));
        assert!(queue.push(Entry {
            queue: 2,
            descriptor: descriptor(20)
        }));
        assert_eq!(queue.pop_for(2), Some(descriptor(20)));
        assert_eq!(queue.pop_for(1), Some(descriptor(10)));
    }

    #[test]
    fn heartbeat_bypasses_a_storm_of_normal_packets() {
        use std::collections::VecDeque;

        let mut normal = (0..10_000).map(descriptor).collect::<VecDeque<_>>();
        let mut priority = PriorityQueue::new();
        let heartbeat = descriptor(usize::MAX);
        assert!(priority.push(Entry {
            queue: 7,
            descriptor: heartbeat,
        }));

        let next = priority.pop_for(7).or_else(|| normal.pop_front());
        assert_eq!(next, Some(heartbeat));
        assert_eq!(normal.len(), 10_000);
    }

    #[test]
    fn failed_signal_rolls_back_and_successful_signal_is_populated() {
        assert!(is_empty());
        assert!(!push_and_signal(9, descriptor(1), || false));
        assert!(is_empty());
        assert!(push_and_signal(9, descriptor(2), || true));
        assert_eq!(pop_for(9), Some(descriptor(2)));
        assert!(is_empty());
    }
}
