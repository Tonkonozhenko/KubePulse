//! Scale limits from `general/resource-browsing`, meant for release builds:
//!
//! ```bash
//! cargo test -p kubepulse-store --release --test scale -- --ignored --nocapture
//! ```

use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use kubepulse_store::{
    Cell, ClusterStore, Row, SortDirection, SortKey, StoreOptions, ViewQuery,
    synthetic::{Burst, SyntheticCluster, pod, pod_columns, pod_kind},
};
use tokio::time::{sleep, timeout};

const PODS: usize = 10_000;
const SORT_FILTER_LIMIT: Duration = Duration::from_millis(100);
const CONVERGE_LIMIT: Duration = Duration::from_secs(2);

fn rows() -> Vec<Arc<Row>> {
    let columns = pod_columns();
    (0..PODS)
        .map(|index| Arc::new(Row::project(&pod(index, 0), &columns)))
        .collect()
}

/// The slowest of several runs, so a single lucky run cannot pass.
fn slowest(rows: &[Arc<Row>], query: &ViewQuery) -> Duration {
    (0..20)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(query.apply(rows));
            start.elapsed()
        })
        .max()
        .unwrap()
}

#[test]
#[ignore = "run in release mode"]
fn sorts_and_filters_ten_thousand_rows_within_the_limit() {
    let rows = rows();
    let queries = [
        ("sort by name", ViewQuery::default()),
        (
            "sort by text column, descending",
            ViewQuery {
                sort: Some((SortKey::Column(2), SortDirection::Descending)),
                ..Default::default()
            },
        ),
        (
            "sort by number column",
            ViewQuery {
                sort: Some((SortKey::Column(1), SortDirection::Ascending)),
                ..Default::default()
            },
        ),
        (
            "filter by text",
            ViewQuery {
                text: "workload-42".into(),
                ..Default::default()
            },
        ),
        (
            "filter by text with no match",
            ViewQuery {
                text: "no-such-pod".into(),
                ..Default::default()
            },
        ),
        (
            "filter by label",
            ViewQuery {
                labels: Some(
                    kubepulse_store::LabelSelector::parse("tier in (front,back),!missing").unwrap(),
                ),
                ..Default::default()
            },
        ),
    ];
    for (name, query) in &queries {
        let took = slowest(&rows, query);
        println!("{name}: {took:?}");
        assert!(took < SORT_FILTER_LIMIT, "{name} took {took:?}");
    }
}

async fn burst_converges(duration: Duration) {
    let burst = Burst {
        changes_per_second: 1_000,
        duration,
    };
    let source = Arc::new(SyntheticCluster::new(PODS, Some(burst)));
    let store = ClusterStore::new(
        source.clone(),
        Arc::new(|_| pod_columns()),
        StoreOptions::default(),
    );
    let view = store.view(&pod_kind(), ViewQuery::default());
    let mut snapshots = view.subscribe();

    timeout(
        Duration::from_secs(30),
        snapshots.wait_for(|s| s.synced && s.rows.len() == PODS),
    )
    .await
    .expect("the initial list should finish")
    .unwrap();
    let expected = (burst.changes_per_second * duration.as_millis() as usize / 1000) as i64;

    let began = Instant::now();
    sleep(duration).await;
    let converged = timeout(duration + CONVERGE_LIMIT, async {
        loop {
            let snapshot = view.snapshot();
            let restarts: i64 = snapshot
                .rows
                .iter()
                .map(|row| match row.cells[1] {
                    Cell::Number(number) => number,
                    _ => 0,
                })
                .sum();
            if restarts >= expected {
                return began.elapsed();
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the view should converge");
    let lag = converged.saturating_sub(duration);
    println!(
        "{} changes in {duration:?}, converged {lag:?} after the burst, {} publications",
        source.changes_sent.load(Ordering::SeqCst),
        view.snapshot().version,
    );
    assert!(lag < CONVERGE_LIMIT, "converged {lag:?} after the burst");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "run in release mode"]
async fn converges_after_a_short_burst() {
    burst_converges(Duration::from_secs(5)).await;
}

/// The full burst from the spec.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "run in release mode"]
async fn converges_after_the_full_thirty_second_burst() {
    burst_converges(Duration::from_secs(30)).await;
}
