//! Process discovery utilities.
//!
//! Find process PIDs by name using `/proc` on Linux or `pgrep` as fallback.

use anyhow::Result;

/// Find the PID of a running process by name.
///
/// On Linux, scans `/proc/*/comm` for exact matches and `/proc/*/cmdline`
/// for substring matches. Falls back to `pgrep` on other platforms.
///
/// Returns an error if:
/// - No matching process is found
/// - Multiple matching processes are found (ambiguous)
pub fn find_pid_by_name(name: &str) -> Result<u32> {
    let pids = find_pids_by_name(name)?;
    match pids.len() {
        0 => anyhow::bail!("no process found matching '{}'", name),
        1 => Ok(pids[0]),
        n => {
            let pid_list: Vec<String> = pids.iter().map(|p| p.to_string()).collect();
            anyhow::bail!(
                "found {} processes matching '{}': [{}]. Use --pid to target a specific one.",
                n,
                name,
                pid_list.join(", ")
            )
        }
    }
}

/// Find all PIDs matching a process name.
///
/// Checks `/proc/PID/comm` (exact match on binary name) and
/// `/proc/PID/cmdline` (substring match on full command).
#[cfg(target_os = "linux")]
fn find_pids_by_name(name: &str) -> Result<Vec<u32>> {
    let mut pids = Vec::new();
    let proc_dir = std::fs::read_dir("/proc")?;

    for entry in proc_dir.flatten() {
        let file_name = entry.file_name();
        let pid_str = file_name.to_string_lossy();
        let pid: u32 = match pid_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Skip PID 1 (init) and our own PID
        if pid <= 1 || pid == std::process::id() {
            continue;
        }

        // Check /proc/PID/comm (exact binary name, max 15 chars)
        let comm_path = format!("/proc/{}/comm", pid);
        if let Ok(comm) = std::fs::read_to_string(&comm_path) {
            let comm = comm.trim();
            if comm == name {
                pids.push(pid);
                continue;
            }
        }

        // Check /proc/PID/cmdline (full command, null-separated)
        let cmdline_path = format!("/proc/{}/cmdline", pid);
        if let Ok(cmdline_bytes) = std::fs::read(&cmdline_path) {
            let cmdline = String::from_utf8_lossy(&cmdline_bytes);
            // argv[0] is the first null-separated field
            let argv0 = cmdline.split('\0').next().unwrap_or("");
            // Match on basename of argv0
            let basename = argv0.rsplit('/').next().unwrap_or(argv0);
            if basename == name {
                pids.push(pid);
            }
        }
    }

    Ok(pids)
}

/// Fallback for non-Linux: use `pgrep`.
#[cfg(not(target_os = "linux"))]
fn find_pids_by_name(name: &str) -> Result<Vec<u32>> {
    let output = std::process::Command::new("pgrep")
        .arg("-x")
        .arg(name)
        .output()?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let pids: Vec<u32> = stdout
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect();

    Ok(pids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_pid_no_match() {
        let result = find_pid_by_name("__heisensim_nonexistent_process_42__");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no process found"));
    }

    #[test]
    fn test_find_pids_returns_vec() {
        // Should not panic, even if empty
        let result = find_pids_by_name("__heisensim_nonexistent_process_42__");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
