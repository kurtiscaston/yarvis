//! Measures what the launcher costs while it sits hidden in the background.
//!
//! Sums CPU time and memory over this process and every process descended
//! from it, which on Windows takes in the WebView2 browser, renderer and GPU
//! processes. On macOS the WebKit helper processes are started by the system
//! rather than by the app, so they are not descendants and are not counted.

use std::collections::HashSet;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

pub struct IdleReport {
    pub seconds: f64,
    /// Percent of one CPU core, averaged over the measurement.
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub processes: usize,
}

impl IdleReport {
    pub fn sentence(&self) -> String {
        let mut text = format!(
            "Idle for {:.0} s: {:.2}% of one core, {:.0} MB across {} {}",
            self.seconds,
            self.cpu_percent,
            self.memory_mb,
            self.processes,
            if self.processes == 1 { "process" } else { "processes" },
        );
        if cfg!(target_os = "macos") {
            text.push_str(" (WebKit helpers not counted; check Activity Monitor)");
        }
        text
    }
}

struct Snapshot {
    cpu_ms: u64,
    memory_bytes: u64,
    processes: usize,
}

/// Blocks for `duration`. Call from a background thread.
pub fn measure(duration: Duration) -> IdleReport {
    let mut system = System::new();
    let before = snapshot(&mut system);
    let started = Instant::now();
    std::thread::sleep(duration);
    let after = snapshot(&mut system);
    let seconds = started.elapsed().as_secs_f64();

    let cpu_ms = after.cpu_ms.saturating_sub(before.cpu_ms) as f64;
    IdleReport {
        seconds,
        cpu_percent: cpu_ms / (seconds * 1_000.0) * 100.0,
        memory_mb: after.memory_bytes as f64 / (1024.0 * 1024.0),
        processes: after.processes,
    }
}

fn snapshot(system: &mut System) -> Snapshot {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );

    let parents: Vec<(Pid, Option<Pid>)> = system
        .processes()
        .iter()
        // On Linux, threads are listed too; their time is already in the process.
        .filter(|(_, process)| process.thread_kind().is_none())
        .map(|(pid, process)| (*pid, process.parent()))
        .collect();
    let family = descendants(Pid::from_u32(std::process::id()), &parents);

    let mut snapshot = Snapshot { cpu_ms: 0, memory_bytes: 0, processes: 0 };
    for pid in &family {
        if let Some(process) = system.process(*pid) {
            snapshot.cpu_ms += process.accumulated_cpu_time();
            snapshot.memory_bytes += process.memory();
            snapshot.processes += 1;
        }
    }
    snapshot
}

/// `root` and everything descended from it.
fn descendants(root: Pid, parents: &[(Pid, Option<Pid>)]) -> HashSet<Pid> {
    let mut family = HashSet::from([root]);
    loop {
        let before = family.len();
        for (pid, parent) in parents {
            if parent.is_some_and(|parent| family.contains(&parent)) {
                family.insert(*pid);
            }
        }
        if family.len() == before {
            return family;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(n: u32) -> Pid {
        Pid::from_u32(n)
    }

    #[test]
    fn finds_children_and_grandchildren_but_not_strangers() {
        let parents = [
            (pid(1), None),
            (pid(10), Some(pid(1))),
            // Listed before its parent, so one pass is not enough.
            (pid(30), Some(pid(20))),
            (pid(20), Some(pid(10))),
            (pid(40), Some(pid(1))),
        ];
        let family = descendants(pid(10), &parents);
        assert_eq!(family, HashSet::from([pid(10), pid(20), pid(30)]));
    }

    #[test]
    fn a_busy_child_shows_up_in_the_measurement() {
        // Burn CPU on a thread of this process for the whole window.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut n = 0u64;
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                n = std::hint::black_box(n.wrapping_add(1));
            }
        });
        let report = measure(Duration::from_millis(400));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        worker.join().unwrap();

        assert!(report.processes >= 1);
        assert!(report.memory_mb > 0.0);
        assert!(report.cpu_percent > 50.0, "measured {:.1}%", report.cpu_percent);
    }
}
