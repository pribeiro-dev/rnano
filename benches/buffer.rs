use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

// Pull in the rnano library modules by path since there is no lib target.
// We compile buffer.rs directly; the types used here match the public API.
//
// NOTE: If the module API changes, update these paths accordingly.

fn make_rope(n_lines: usize) -> ropey::Rope {
    let mut s = String::with_capacity(n_lines * 40);
    for i in 0..n_lines {
        s.push_str(&format!("line {:>6}: hello world from rnano bench\n", i));
    }
    ropey::Rope::from_str(&s)
}

// --- rope append --------------------------------------------------------------

fn bench_rope_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("rope/append");
    for &n in &[100usize, 1_000, 10_000] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| {
                let mut rope = ropey::Rope::new();
                for i in 0..n {
                    let ch_pos = rope.len_chars();
                    rope.insert(ch_pos, &format!("line {i}\n"));
                }
                black_box(rope.len_chars())
            });
        });
    }
    group.finish();
}

// --- rope random insert -------------------------------------------------------

fn bench_rope_random_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("rope/random_insert");
    for &n in &[1_000usize, 10_000] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| {
                let mut rope = make_rope(n);
                // Insert at the middle on each iteration.
                let mid = rope.len_chars() / 2;
                rope.insert(mid, "X");
                black_box(rope.len_chars())
            });
        });
    }
    group.finish();
}

// --- line_to_char (viewport scroll lookup) ------------------------------------

fn bench_line_to_char(c: &mut Criterion) {
    let mut group = c.benchmark_group("rope/line_to_char");
    for &n in &[1_000usize, 100_000] {
        let rope = make_rope(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &rope, |b, rope| {
            b.iter(|| {
                let mid = rope.len_lines() / 2;
                black_box(rope.line_to_char(mid))
            });
        });
    }
    group.finish();
}

// --- to_string (save path) ----------------------------------------------------

fn bench_to_string(c: &mut Criterion) {
    let mut group = c.benchmark_group("rope/to_string");
    for &n in &[100usize, 10_000] {
        let rope = make_rope(n);
        let bytes = rope.len_bytes() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_with_input(BenchmarkId::from_parameter(n), &rope, |b, rope| {
            b.iter(|| black_box(rope.to_string()));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_rope_append,
    bench_rope_random_insert,
    bench_line_to_char,
    bench_to_string
);
criterion_main!(benches);
