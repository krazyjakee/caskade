//! `cargo bench -p caskade-installer --all-features --bench installer`.
//! Assertions live in tests/performance.rs; these benchmarks retain Criterion
//! distributions for trends. Workload construction/cleanup is not measured.
#[cfg(target_os = "linux")]
mod support;
#[cfg(target_os = "linux")]
mod benchmarks {
    use super::support;
    use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
    use std::{hint::black_box, time::Duration};
    fn installer(c: &mut Criterion) {
        let probe_dir = support::temporary("caskade-bench-probe-").unwrap();
        let path = support::sparse_installer(probe_dir.path()).unwrap();
        c.bench_function("probe/20-gib-sparse-inno", |b| {
            b.iter(|| black_box(support::probe(&path).unwrap().0));
        });
        let tree = support::temporary("caskade-bench-discovery-").unwrap();
        support::discovery_tree(tree.path()).unwrap();
        c.bench_function("discovery/100000-files", |b| {
            b.iter(|| black_box(support::discovery(tree.path()).unwrap()));
        });
        let archive_dir = support::temporary("caskade-bench-tar-").unwrap();
        let archive = archive_dir.path().join("throughput.tar");
        support::sparse_tar(&archive, support::THROUGHPUT_BYTES).unwrap();
        c.bench_function("extract/tar-128-mib", |b| {
            b.iter_batched(
                || support::temporary("caskade-bench-output-").unwrap(),
                |output| {
                    black_box(
                        support::extract(
                            &archive,
                            &output.path().join("game"),
                            support::THROUGHPUT_BYTES,
                        )
                        .unwrap(),
                    );
                    output
                },
                BatchSize::PerIteration,
            );
        });
        // The default nightly RSS assertion always extracts 10 GiB in its own
        // process. Opt in here for repeated Criterion timing of that workload.
        if std::env::var_os("CASKADE_BENCH_LARGE").is_some() {
            let archive = archive_dir.path().join("ten-gib.tar");
            support::sparse_tar(&archive, support::RSS_BYTES).unwrap();
            c.bench_function("extract/tar-10-gib", |b| {
                b.iter_batched(
                    || support::temporary("caskade-bench-large-").unwrap(),
                    |output| {
                        black_box(
                            support::extract(
                                &archive,
                                &output.path().join("game"),
                                support::RSS_BYTES,
                            )
                            .unwrap(),
                        );
                        output
                    },
                    BatchSize::PerIteration,
                );
            });
        }
    }
    criterion_group! { name = benches; config = Criterion::default().sample_size(10).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(5)); targets = installer }
    criterion_main!(benches);
    pub fn run() {
        main();
    }
}
#[cfg(target_os = "linux")]
fn main() {
    benchmarks::run();
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("T10 host performance benchmarks run on the Linux native runner");
}
