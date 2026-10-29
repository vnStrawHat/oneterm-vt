use compare_harness::MemoryPoints;

use super::*;

fn judgement(status: Status) -> Judgement {
    Judgement {
        status,
        first_difference: None,
        error: None,
    }
}

fn timed(samples: &[u64]) -> TimedPair {
    TimedPair {
        samples_ns: samples.to_vec(),
        hashes: vec!["h".into()],
        bytes: 32 << 20,
        error: None,
    }
}

fn verified(base: u64, steady: u64, peak: u64) -> VerifyOutput {
    VerifyOutput {
        dumps: Vec::new(),
        final_sha256: "h".into(),
        memory: MemoryPoints { base, steady, peak },
        scrollback_rows: None,
    }
}

#[test]
fn a_timed_pair_gets_rate_spread_and_memory_above_the_base() {
    let samples: Vec<u64> = (0..15).map(|n| 500_000_000 + n * 1_000_000).collect();
    let output = verified(10 << 20, 74 << 20, 138 << 20);
    let row = assemble_row(
        "oneterm",
        "plain_ascii",
        &judgement(Status::Ok),
        Some(&output),
        Some(&timed(&samples)),
    );
    // 32 MiB in the 507 ms median.
    let rate = row.mib_per_second.unwrap();
    assert!((rate - 32.0 / 0.507).abs() < 1e-6, "{rate}");
    assert!((row.ns_per_byte.unwrap() * (32 << 20) as f64 - 507_000_000.0).abs() < 1.0);
    assert!(row.spread_percent.unwrap() > 0.0);
    assert_eq!(row.samples_ns.len(), 15);
    assert_eq!(row.steady_mib, Some(64.0));
    assert_eq!(row.peak_mib, Some(128.0));
}

#[test]
fn diverged_and_unverified_pairs_keep_their_numbers_failed_and_unsupported_do_not() {
    let samples = [1_000_000_000; 3];
    for status in [Status::Diverged, Status::Unverified, Status::Accepted] {
        let row = assemble_row("x", "f", &judgement(status), None, Some(&timed(&samples)));
        assert_eq!(row.mib_per_second, Some(32.0), "{status:?}");
    }
    for status in [Status::Failed, Status::Unsupported] {
        let row = assemble_row("x", "f", &judgement(status), None, Some(&timed(&samples)));
        assert_eq!(row.mib_per_second, None, "{status:?}");
        assert!(row.samples_ns.is_empty());
    }
}

#[test]
fn memory_below_the_base_clamps_to_zero() {
    let output = verified(100 << 20, 90 << 20, 95 << 20);
    let row = assemble_row("x", "f", &judgement(Status::Ok), Some(&output), None);
    assert_eq!((row.steady_mib, row.peak_mib), (Some(0.0), Some(0.0)));
}
