//! SpacetimeDB's `crates/bench` generic matrix, for Interstice.
//!
//! Ids read `<target>/<mem|disk>/<op>/<schema>/<index>/<sizes>`, as theirs.
//! Every sample is one transaction; setup (clearing, preloading) is not
//! timed. Run: `cargo bench -p interstice-bench --bench generic [filter]`.

use criterion::{Criterion, criterion_group, criterion_main};
use interstice_bench::data::{self, IndexStrategy, Schema, TableId, XorShift};
use interstice_bench::targets::{BenchDatabase, ModuleTarget, RawTarget, SqliteTarget};
use std::time::{Duration, Instant};

const LOAD: u32 = 2048;
const COUNT: u32 = 256;
const BUCKETS: u64 = 8;
const SCHEMAS: [Schema; 2] = [Schema::U32U64Str, Schema::U32U64U64];

fn targets() -> Vec<(Box<dyn BenchDatabase>, Vec<bool>)> {
    vec![
        (Box::new(RawTarget::new()), vec![false]),
        (Box::new(ModuleTarget::new()), vec![false, true]),
        (Box::new(SqliteTarget::new(false)), vec![false]),
        (Box::new(SqliteTarget::new(true)), vec![true]),
    ]
}

fn id(target: &dyn BenchDatabase, disk: bool, rest: String) -> String {
    format!("{}/{}/{rest}", target.name(), if disk { "disk" } else { "mem" })
}

fn generic(c: &mut Criterion) {
    let mut targets = targets();
    for (target, disks) in &mut targets {
        let target = target.as_mut();
        if target.name() == "module" {
            c.bench_function("module/mem/empty", |b| b.iter(|| target.empty()));
        }
        for &disk in disks.iter() {
            for schema in SCHEMAS {
                for index in [IndexStrategy::Unique0, IndexStrategy::BtreeEachColumn] {
                    let table = TableId { schema, index, disk };
                    let shape = format!("{}/{}", schema.label(), index.label());
                    let sizes = |load: u32, count: u32| format!("{shape}/load={load}/count={count}");

                    // insert_bulk: COUNT new rows into a table holding LOAD.
                    c.bench_function(&id(target, disk, format!("insert_bulk/{}", sizes(LOAD, COUNT))), |b| {
                        b.iter_custom(|iters| {
                            let mut rng = XorShift::new();
                            let preload = data::rows(&mut rng, 0, LOAD, BUCKETS);
                            let fresh = data::rows(&mut rng, LOAD, COUNT, BUCKETS);
                            let mut total = Duration::ZERO;
                            for _ in 0..iters {
                                target.clear(table);
                                target.insert_bulk(table, &preload);
                                let start = Instant::now();
                                target.insert_bulk(table, &fresh);
                                total += start.elapsed();
                            }
                            total
                        })
                    });

                    match index {
                        IndexStrategy::Unique0 => {
                            // update_bulk: COUNT rows of LOAD, by id.
                            let mut rng = XorShift::new();
                            target.clear(table);
                            target.insert_bulk(table, &data::rows(&mut rng, 0, LOAD, BUCKETS));
                            c.bench_function(&id(target, disk, format!("update_bulk/{}", sizes(LOAD, COUNT))), |b| {
                                b.iter(|| target.update_bulk(table, COUNT))
                            });

                            // iterate: every row of a COUNT-row table.
                            let mut rng = XorShift::new();
                            target.clear(table);
                            target.insert_bulk(table, &data::rows(&mut rng, 0, COUNT, BUCKETS));
                            c.bench_function(&id(target, disk, format!("iterate/{shape}/count={COUNT}")), |b| {
                                b.iter(|| target.iterate(table))
                            });
                        }
                        IndexStrategy::BtreeEachColumn => {
                            // filter: column 3 equal to one bucket of LOAD rows.
                            let mut rng = XorShift::new();
                            target.clear(table);
                            target.insert_bulk(table, &data::rows(&mut rng, 0, LOAD, BUCKETS));
                            let mut bucket = 0;
                            c.bench_function(&id(target, disk, format!("filter/{shape}/load={LOAD}/buckets={BUCKETS}")), |b| {
                                b.iter(|| {
                                    bucket = (bucket + 1) % BUCKETS;
                                    target.filter(table, bucket)
                                })
                            });
                        }
                    }
                    target.clear(table);
                }
            }
        }
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = generic
}
criterion_main!(benches);
