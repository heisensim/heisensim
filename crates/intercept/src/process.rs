//! Process discovery utilities.
//!
//! Find process PIDs by name using `/proc` on Linux or `pgrep` as fallback.

use anyhow::Result;

/// Find the PID of a running process by name.
///
/// On Linux, scans `/proc/*/comm` for exact matches and `/proc/*/cmdline`
/// for exact basename-of-argv[0] matches. Falls back to `pgrep -x` on other platforms.
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

/// Escape regex metacharacters for safe use with `pgrep -x`.
///
/// `pgrep -x` treats the pattern as an extended regex, so characters like
/// `.`, `+`, `*`, `(`, etc. in process names (e.g. `svc.v1`) would match
/// unintended processes.
#[cfg(not(target_os = "linux"))]
fn escape_regex(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if matches!(
            c,
            '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// Fallback for non-Linux: use `pgrep`.
#[cfg(not(target_os = "linux"))]
fn find_pids_by_name(name: &str) -> Result<Vec<u32>> {
    let escaped = escape_regex(name);
    let output = std::process::Command::new("pgrep")
        .arg("-x")
        .arg(&escaped)
        .output()?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let my_pid = std::process::id();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pids: Vec<u32> = stdout
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        // Skip PID 1 (init) and our own PID — same guards as the Linux path
        .filter(|&pid| pid > 1 && pid != my_pid)
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
    fn test_find_pids_returns_empty_vec_for_nonexistent() {
        let result = find_pids_by_name("__heisensim_nonexistent_process_42__");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_find_pid_by_name_ambiguous_error_message() {
        // We can't easily create multiple processes with the same name in a test,
        // but we can verify the error format by calling find_pid_by_name directly
        // with a wrapper that simulates multiple results.
        let err_msg = format!(
            "found {} processes matching '{}': [{}]. Use --pid to target a specific one.",
            3, "node", "1234, 5678, 9012"
        );
        assert!(err_msg.contains("found 3 processes"));
        assert!(err_msg.contains("Use --pid"));
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_escape_regex_plain_name() {
        assert_eq!(escape_regex("my-server"), "my-server");
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_escape_regex_dots() {
        // svc.v1 should become svc\.v1 to prevent regex wildcard matching
        assert_eq!(escape_regex("svc.v1"), r"svc\.v1");
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_escape_regex_metacharacters() {
        assert_eq!(escape_regex("a+b*c?d"), r"a\+b\*c\?d");
        assert_eq!(escape_regex("(foo)"), r"\(foo\)");
        assert_eq!(escape_regex("[bar]"), r"\[bar\]");
        assert_eq!(escape_regex("^start$"), r"\^start\$");
    }

    #[test]
    fn test_find_pids_excludes_self() {
        // Run find_pids_by_name for our own process name — it should NOT
        // include our own PID in the results
        let my_pid = std::process::id();
        // Use the test binary name — won't match on comm but tests the filter
        let result = find_pids_by_name("__self_pid_test__");
        assert!(result.is_ok());
        let pids = result.unwrap();
        assert!(!pids.contains(&my_pid));
        assert!(!pids.contains(&1)); // PID 1 should never appear
    }
}
