use crate::seed::SimSeed;
use crate::types::{NodeId, VirtualTime};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::{BTreeMap, HashSet};

/// A message sent over the virtual network.
#[derive(Debug, Clone)]
pub struct Message {
    pub from: NodeId,
    pub to: NodeId,
    pub data: Vec<u8>,
    pub sent_at: VirtualTime,
}

/// Defines a network partition between two sets of nodes.
#[derive(Debug, Clone)]
pub struct Partition {
    pub id: u64,
    pub nodes_a: HashSet<NodeId>,
    pub nodes_b: HashSet<NodeId>,
}

/// The virtual network mediating communication between simulated nodes.
pub struct VirtualNetwork {
    pending_messages: BTreeMap<VirtualTime, Vec<Message>>,
    active_partitions: Vec<Partition>,
    drop_probability: f64,
    delay_range_ms: (u64, u64),
    rng: StdRng,
    next_partition_id: u64,
}

impl VirtualNetwork {
    /// Creates a new `VirtualNetwork` using the provided seed.
    pub fn new(seed: SimSeed) -> Self {
        Self {
            pending_messages: BTreeMap::new(),
            active_partitions: Vec::new(),
            drop_probability: 0.0,
            delay_range_ms: (1, 10), // Default 1-10ms delay
            rng: StdRng::seed_from_u64(seed.0),
            next_partition_id: 0,
        }
    }

    /// Sets the probability (0.0 to 1.0) that a message will be dropped.
    pub fn set_drop_probability(&mut self, p: f64) {
        self.drop_probability = p.clamp(0.0, 1.0);
    }

    /// Sets the min and max delay range in milliseconds for messages.
    pub fn set_delay_range(&mut self, min: u64, max: u64) {
        self.delay_range_ms = (min, max.max(min));
    }

    /// Adds a network partition between two sets of nodes. Returns the partition ID.
    pub fn add_partition(&mut self, nodes_a: HashSet<NodeId>, nodes_b: HashSet<NodeId>) -> u64 {
        let id = self.next_partition_id;
        self.next_partition_id += 1;
        self.active_partitions.push(Partition {
            id,
            nodes_a,
            nodes_b,
        });
        id
    }

    /// Removes a network partition by ID.
    pub fn remove_partition(&mut self, id: u64) {
        self.active_partitions.retain(|p| p.id != id);
    }

    /// Checks if communication between two nodes is partitioned.
    fn is_partitioned(&self, from: NodeId, to: NodeId) -> bool {
        for partition in &self.active_partitions {
            if (partition.nodes_a.contains(&from) && partition.nodes_b.contains(&to))
                || (partition.nodes_b.contains(&from) && partition.nodes_a.contains(&to))
            {
                return true;
            }
        }
        false
    }

    /// Sends a message, scheduling it for future delivery or dropping it.
    pub fn send(&mut self, current_time: VirtualTime, from: NodeId, to: NodeId, data: Vec<u8>) {
        if self.is_partitioned(from, to) {
            // Drop message silently due to partition
            return;
        }

        if self.rng.random::<f64>() < self.drop_probability {
            // Drop message randomly
            return;
        }

        let delay_ms = self
            .rng
            .random_range(self.delay_range_ms.0..=self.delay_range_ms.1);
        let deliver_at = current_time + VirtualTime::from_millis(delay_ms);

        let msg = Message {
            from,
            to,
            data,
            sent_at: current_time,
        };

        self.pending_messages
            .entry(deliver_at)
            .or_default()
            .push(msg);
    }

    /// Retrieves all messages that are ready to be delivered at or before the current time.
    pub fn deliver_ready(&mut self, current_time: VirtualTime) -> Vec<(NodeId, Message)> {
        let mut ready = Vec::new();

        let mut remaining = BTreeMap::new();

        for (time, msgs) in std::mem::take(&mut self.pending_messages) {
            if time <= current_time {
                for msg in msgs {
                    ready.push((msg.to, msg));
                }
            } else {
                remaining.insert(time, msgs);
            }
        }

        self.pending_messages = remaining;
        ready
    }
}
