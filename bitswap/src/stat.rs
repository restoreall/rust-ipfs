use std::sync::atomic::{AtomicU64, Ordering};

/// Bitswap statistics.
#[derive(Debug, Default)]
pub struct Stats {
    pub sent_blocks: AtomicU64,
    pub sent_data: AtomicU64,
    pub received_blocks: AtomicU64,
    pub received_data: AtomicU64,
    pub duplicate_blocks: AtomicU64,
    pub duplicate_data: AtomicU64,
}

impl Stats {
    pub fn update_outgoing(&self, num_blocks: u64) {
        self.sent_blocks.fetch_add(num_blocks, Ordering::Relaxed);
    }

    pub fn update_incoming_unique(&self, bytes: u64) {
        self.received_blocks.fetch_add(1, Ordering::Relaxed);
        self.received_data.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn update_incoming_duplicate(&self, bytes: u64) {
        self.duplicate_blocks.fetch_add(1, Ordering::Relaxed);
        self.duplicate_data.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn add_assign(&self, other: &Stats) {
        self.sent_blocks
            .fetch_add(other.sent_blocks.load(Ordering::Relaxed), Ordering::Relaxed);
        self.sent_data
            .fetch_add(other.sent_data.load(Ordering::Relaxed), Ordering::Relaxed);
        self.received_blocks.fetch_add(
            other.received_blocks.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.received_data.fetch_add(
            other.received_data.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.duplicate_blocks.fetch_add(
            other.duplicate_blocks.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.duplicate_data.fetch_add(
            other.duplicate_data.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }
}
