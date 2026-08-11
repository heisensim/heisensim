use crate::types::{NodeId, ProcessId, VirtualTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The state of a simulated process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessState {
    Starting,
    Running,
    Sleeping,
    Blocked,
    Crashed,
    Killed,
}

/// A tracked process in the simulation.
#[derive(Debug, Clone)]
pub struct TrackedProcess {
    pub pid: ProcessId,
    pub node_id: NodeId,
    pub name: String,
    pub state: ProcessState,
    pub started_at: VirtualTime,
}

/// Table tracking all processes across all simulated nodes.
#[derive(Debug, Default)]
pub struct ProcessTable {
    processes: HashMap<ProcessId, TrackedProcess>,
    next_pid: u32,
}

impl ProcessTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new process and returns its ID.
    pub fn register(
        &mut self,
        node_id: NodeId,
        name: String,
        started_at: VirtualTime,
    ) -> ProcessId {
        let pid = ProcessId(self.next_pid);
        self.next_pid += 1;

        let process = TrackedProcess {
            pid,
            node_id,
            name,
            state: ProcessState::Starting,
            started_at,
        };

        self.processes.insert(pid, process);
        pid
    }

    /// Marks a process as killed.
    pub fn kill(&mut self, pid: ProcessId) -> bool {
        if let Some(proc) = self.processes.get_mut(&pid) {
            proc.state = ProcessState::Killed;
            true
        } else {
            false
        }
    }

    /// Marks a process as crashed.
    pub fn crash(&mut self, pid: ProcessId) -> bool {
        if let Some(proc) = self.processes.get_mut(&pid) {
            proc.state = ProcessState::Crashed;
            true
        } else {
            false
        }
    }

    /// Gets a reference to a process by ID.
    pub fn get(&self, pid: ProcessId) -> Option<&TrackedProcess> {
        self.processes.get(&pid)
    }

    /// Lists all processes running on a given node.
    pub fn list_by_node(&self, node_id: NodeId) -> Vec<&TrackedProcess> {
        self.processes
            .values()
            .filter(|p| p.node_id == node_id)
            .collect()
    }

    /// Lists all currently alive processes.
    pub fn list_alive(&self) -> Vec<&TrackedProcess> {
        self.processes
            .values()
            .filter(|p| {
                matches!(
                    p.state,
                    ProcessState::Starting
                        | ProcessState::Running
                        | ProcessState::Sleeping
                        | ProcessState::Blocked
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_table() {
        let mut table = ProcessTable::new();
        let node = NodeId(1);

        let pid1 = table.register(node, "proc1".to_string(), VirtualTime(0));
        let pid2 = table.register(node, "proc2".to_string(), VirtualTime(10));

        assert_eq!(table.list_by_node(node).len(), 2);
        assert_eq!(table.list_alive().len(), 2);

        assert!(table.crash(pid1));
        assert_eq!(table.list_alive().len(), 1);

        assert!(table.kill(pid2));
        assert_eq!(table.list_alive().len(), 0);

        assert!(!table.crash(ProcessId(999))); // invalid pid

        let proc = table.get(pid1).unwrap();
        assert_eq!(proc.state, ProcessState::Crashed);
        assert_eq!(proc.name, "proc1");
    }
}
