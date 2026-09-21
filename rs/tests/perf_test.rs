// Machine-INDEPENDENT performance regression guards.
//
// Building an instance (the engine, the base grammar and the operator
// table) dominates a parse, so the contract is: build ONE instance and
// reuse it. These tests compare the two paths on the SAME machine in the
// SAME run, so a slow box cannot make them flaky, and there is
// deliberately NO wall-clock budget.

mod common;

use std::time::Instant;

use tabnas_expr::{make, parse, parse_with};

const SRC: &str = "1+2*3";

/// The convenience [`parse`] must reuse one lazily built instance rather
/// than rebuilding the grammar per call. A rebuild-per-call parse is many
/// times slower than instance reuse; the allowance is for scheduling
/// noise, not for a rebuild.
#[test]
fn parse_reuses_its_instance() {
    const N: usize = 2000;

    // Warm both paths so the comparison is steady-state.
    for _ in 0..100 {
        parse(SRC).expect("parses");
    }
    let parser = make();
    for _ in 0..100 {
        parse_with(&parser, SRC).expect("parses");
    }

    let start = Instant::now();
    for _ in 0..N {
        parse(SRC).expect("parses");
    }
    let convenience = start.elapsed();

    let start = Instant::now();
    for _ in 0..N {
        parse_with(&parser, SRC).expect("parses");
    }
    let reuse = start.elapsed();

    assert!(
        convenience <= 4 * reuse,
        "parse() appears to rebuild the grammar on every call: {N} calls took {convenience:?} \
         against {reuse:?} reusing one instance. Keep the lazily built default instance."
    );
    println!("perf: parse()={convenience:?} reuse={reuse:?}");
}

/// Reusing one instance is dramatically cheaper than building a fresh one
/// per parse. A small ratio would mean the build is free, which it is not,
/// so this also confirms the test exercises the real cost.
#[test]
fn reusing_an_instance_beats_rebuilding_it() {
    const N: usize = 200;

    for _ in 0..20 {
        parse_with(&make(), SRC).expect("parses");
    }
    let parser = make();
    for _ in 0..100 {
        parse_with(&parser, SRC).expect("parses");
    }

    let start = Instant::now();
    for _ in 0..N {
        parse_with(&make(), SRC).expect("parses");
    }
    let rebuild = start.elapsed();

    let start = Instant::now();
    for _ in 0..N {
        parse_with(&parser, SRC).expect("parses");
    }
    let reuse = start.elapsed();

    assert!(
        rebuild > 2 * reuse,
        "expected building a fresh instance per parse to be more than twice as slow as reusing \
         one over {N} parses (the grammar build dominates), but the rebuild took {rebuild:?} \
         against {reuse:?}"
    );
    println!("perf: rebuild-per-parse={rebuild:?} reuse-one={reuse:?} ({N} parses)");
}
