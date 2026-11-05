//! Measures what the cache allocates per pod, so it must stay the only test in
//! this binary: a counting allocator sees every thread in the process.

use std::{alloc::System, sync::Arc};

use kubepulse_store::{
    Row,
    synthetic::{pod, pod_columns},
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static ALLOCATOR: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const PODS: usize = 10_000;
const LIMIT_BYTES_PER_POD: usize = 800;

#[test]
fn rows_stay_compact_and_interned_strings_are_freed() {
    let columns = pod_columns();
    let objects: Vec<_> = (0..PODS).map(|index| pod(index, 0)).collect();

    let region = Region::new(ALLOCATOR);
    let rows: Vec<Arc<Row>> = objects
        .iter()
        .map(|o| Arc::new(Row::project(o, &columns)))
        .collect();
    let stats = region.change();
    let retained = stats
        .bytes_allocated
        .saturating_sub(stats.bytes_deallocated);
    let per_pod = retained / PODS;
    println!(
        "{per_pod} bytes per pod, {} MiB for {PODS} pods",
        retained / (1024 * 1024)
    );
    assert!(per_pod <= LIMIT_BYTES_PER_POD, "{per_pod} bytes per pod");

    // Pods that go away must not leave their strings behind.
    drop(rows);
    let after_drop = region.change();
    let leaked = after_drop
        .bytes_allocated
        .saturating_sub(after_drop.bytes_deallocated);
    println!(
        "{} bytes per pod retained after dropping every row",
        leaked / PODS
    );
    assert!(
        leaked / PODS < 100,
        "{} bytes per pod left behind",
        leaked / PODS
    );
}
