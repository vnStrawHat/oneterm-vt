use super::*;

#[test]
fn median_and_percentiles_of_fifteen_known_samples() {
    // Shuffled 100, 200, ... 1500 ns.
    let samples = [
        800, 100, 1500, 300, 1200, 700, 200, 1400, 500, 900, 1100, 400, 1000, 600, 1300,
    ];
    let stats = summarize(&samples).unwrap();
    assert_eq!(stats.median_ns, 800.0);
    // Nearest rank: index round(0.1 * 14) = 1 and round(0.9 * 14) = 13.
    assert_eq!(stats.p10_ns, 200.0);
    assert_eq!(stats.p90_ns, 1400.0);
    assert_eq!(stats.spread_percent, 150.0);
}

#[test]
fn even_count_takes_the_middle_pair_mean() {
    assert_eq!(summarize(&[10, 20, 30, 40]).unwrap().median_ns, 25.0);
}

#[test]
fn empty_set_has_no_stats() {
    assert_eq!(summarize(&[]), None);
}

#[test]
fn zero_duration_stays_finite() {
    let stats = summarize(&[0, 0, 0]).unwrap();
    assert_eq!(stats.spread_percent, 0.0);
    assert!(mib_per_second(1 << 20, stats.median_ns).is_finite());
}

#[test]
fn rate_is_mib_over_seconds() {
    assert_eq!(mib_per_second(1 << 20, 1e9), 1.0);
    assert_eq!(mib_per_second(32 << 20, 0.5e9), 64.0);
}
