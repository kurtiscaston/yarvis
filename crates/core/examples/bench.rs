//! Times the search engine alone, with no UI in the way.
//!
//!     cargo run --release -p yarvis-core --example bench
//!
//! Each query is typed one character at a time, the way a person would, so
//! every prefix is searched. The index is far larger than a real machine's.

use yarvis_core::search::{sort_items, Engine};
use yarvis_core::usage::Usage;
use yarvis_core::Item;
use std::time::Instant;

const ITEMS: usize = 5_000;
const LIMIT: usize = 50;
const ROUNDS: usize = 200;
const QUERIES: &[&str] = &["chrome", "vs code", "calc", "term", "photo edit", "sys pref", "xq"];

fn main() {
    let mut engine = Engine::new(synthetic_items(ITEMS));
    let mut usage = Usage::in_memory();
    for n in (0..ITEMS).step_by(37) {
        usage.record(&format!("app:/apps/{n}"), 1_000);
    }

    let mut samples = Vec::new();
    let mut hits = 0;
    for _ in 0..ROUNDS {
        for query in QUERIES {
            for end in 1..=query.len() {
                let start = Instant::now();
                hits += engine.search(&query[..end], LIMIT, &usage, 2_000).len();
                samples.push(start.elapsed().as_secs_f64() * 1e6);
            }
        }
    }
    samples.sort_by(f64::total_cmp);

    let at = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
    println!("{} items, top {LIMIT}, {} searches ({hits} hits returned)", engine.len(), samples.len());
    println!("median {:>7.0} µs", at(0.50));
    println!("p95    {:>7.0} µs", at(0.95));
    println!("p99    {:>7.0} µs", at(0.99));
    println!("worst  {:>7.0} µs", at(1.00));
}

fn synthetic_items(count: usize) -> Vec<Item> {
    const FIRST: &[&str] = &[
        "Google", "Visual Studio", "Adobe", "Microsoft", "System", "Photo", "Disk", "Network",
        "Terminal", "Calendar", "Calculator", "Music", "Video", "Remote", "Font", "Color",
    ];
    const SECOND: &[&str] = &[
        "Chrome", "Code", "Editor", "Preferences", "Utility", "Manager", "Viewer", "Studio",
        "Player", "Monitor", "Console", "Sync", "Backup", "Notes", "Mail", "Maps",
    ];
    let mut items: Vec<Item> = (0..count)
        .map(|n| {
            let title = format!("{} {} {}", FIRST[n % FIRST.len()], SECOND[(n / FIRST.len()) % SECOND.len()], n);
            Item::app(title, format!("/apps/{n}"))
        })
        .collect();
    sort_items(&mut items);
    items
}
