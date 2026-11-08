extern crate alloc;
#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/name/mod.rs"]
mod name;
use hashbrown::HashMap;
use name::{Arena, Key, Registry};
use std::{hint::black_box, time::Instant};

/// Exploratory, single-threaded storage churn with a 64-byte synthetic holder.
/// This excludes deadline queues, locks, Task lifetime and target CPU effects.
fn main() {
    const ROUNDS: usize = 200_000;
    for count in [16, 256, 4096] {
        let mut samples = [Vec::new(), Vec::new(), Vec::new()];
        for _ in 0..7 {
            let mut map = HashMap::new();
            let mut indexed = Registry::default();
            let mut arena = Arena::<[u64; 8], ()>::new();
            let mut keys: Vec<Key<()>> = Vec::new();
            let mut ids = Vec::new();
            for id in 0..count {
                map.insert(id, [id as u64; 8]);
                indexed.insert(id, [id as u64; 8]).unwrap();
                let key = arena.reserve().unwrap();
                arena.publish(key, [id as u64; 8]).unwrap();
                keys.push(key);
                ids.push(id);
            }
            let start = Instant::now();
            for i in 0..ROUNDS {
                let at = i % count;
                black_box(map.get(&ids[at]).unwrap());
                let value = map.remove(&ids[at]).unwrap();
                ids[at] = count + i;
                map.insert(ids[at], black_box(value));
            }
            samples[0].push(start.elapsed().as_nanos() as f64 / ROUNDS as f64);
            ids.iter_mut().enumerate().for_each(|(i, id)| *id = i);
            let start = Instant::now();
            for i in 0..ROUNDS {
                let at = i % count;
                black_box(indexed.get(ids[at]).unwrap());
                let value = indexed.remove(ids[at]).unwrap();
                ids[at] = count + i;
                indexed.insert(ids[at], black_box(value)).unwrap();
            }
            samples[1].push(start.elapsed().as_nanos() as f64 / ROUNDS as f64);
            let start = Instant::now();
            for i in 0..ROUNDS {
                let at = i % count;
                black_box(arena.get(keys[at]).unwrap());
                let value = arena.remove(keys[at]).unwrap();
                keys[at] = arena.reserve().unwrap();
                arena.publish(keys[at], black_box(value)).unwrap();
            }
            samples[2].push(start.elapsed().as_nanos() as f64 / ROUNDS as f64);
        }
        for sample in &mut samples {
            sample.sort_by(f64::total_cmp);
        }
        println!(
            "holders={count} ns/churn median: HashMap={:.1} indexed-Arena={:.1} direct-Arena={:.1}; key={}B ticket={}B",
            samples[0][3],
            samples[1][3],
            samples[2][3],
            size_of::<Key<()>>(),
            size_of::<u64>()
        );
    }
}
