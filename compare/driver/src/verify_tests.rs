use compare_harness::{Dump, MemoryPoints};

use super::*;

pub(crate) fn dump_with(row: &str) -> Dump {
    Dump {
        cursor: (0, 0),
        rows: vec![row.to_string()],
        history: Vec::new(),
        attrs: Vec::new(),
        links: Vec::new(),
    }
}

pub(crate) fn output(row: &str) -> VerifyOutput {
    let dumps = vec![dump_with(row); 4];
    VerifyOutput {
        final_sha256: dumps[3].sha256(),
        dumps,
        memory: MemoryPoints::default(),
        scrollback_rows: None,
    }
}

fn golden(row: &str) -> GoldenFile {
    GoldenFile {
        fixture: "plain_ascii".into(),
        corpus_version: 1,
        fixture_sha256: "x".into(),
        agreeing_engines: vec!["a".into(), "b".into(), "c".into()],
        dumps: output(row).dumps,
    }
}

fn input<'a>(
    verify: Option<&'a Ran<VerifyOutput>>,
    golden: Option<&'a GoldenFile>,
    timed: Timed<'a>,
    accepted: &'a [AcceptedEntry],
) -> PairInput<'a> {
    PairInput {
        engine: "vt100",
        fixture: "plain_ascii",
        supported: true,
        verify,
        golden,
        timed,
        accepted,
    }
}

#[test]
fn ok_when_the_dumps_equal_the_golden_and_the_timed_hashes_tie() {
    let ran = Ran::Ok(output("same"));
    let hashes = vec![output("same").final_sha256; 3];
    let judged = judge(&input(
        Some(&ran),
        Some(&golden("same")),
        Timed::Hashes(&hashes),
        &[],
    ));
    assert_eq!(judged.status, Status::Ok);
    assert!(judged.first_difference.is_none());
}

#[test]
fn accepted_only_for_the_exact_engine_fixture_and_hash() {
    let ran = Ran::Ok(output("other"));
    let entry = |engine: &str, fixture: &str, hash: &str| AcceptedEntry {
        engine: engine.into(),
        fixture: fixture.into(),
        final_sha256: hash.into(),
        reason: "width table".into(),
    };
    let hash = output("other").final_sha256;
    let g = golden("same");
    let status = |entries: &[AcceptedEntry]| {
        judge(&input(Some(&ran), Some(&g), Timed::NotRun, entries)).status
    };
    assert_eq!(
        status(&[entry("vt100", "plain_ascii", &hash)]),
        Status::Accepted
    );
    assert_eq!(
        status(&[entry("rio", "plain_ascii", &hash)]),
        Status::Diverged
    );
    assert_eq!(
        status(&[entry("vt100", "cjk_wide", &hash)]),
        Status::Diverged
    );
    assert_eq!(
        status(&[entry("vt100", "plain_ascii", "stale")]),
        Status::Diverged
    );
    assert_eq!(status(&[]), Status::Diverged);
}

#[test]
fn diverged_names_the_first_difference() {
    let ran = Ran::Ok(output("hellp"));
    let judged = judge(&input(
        Some(&ran),
        Some(&golden("hello")),
        Timed::NotRun,
        &[],
    ));
    assert_eq!(judged.status, Status::Diverged);
    assert_eq!(
        judged.first_difference.as_deref(),
        Some("checkpoint 0, rows[0] col 4: expected \"o\" got \"p\"")
    );
}

#[test]
fn unsupported_failed_and_unverified_rows() {
    let ran = Ran::Ok(output("same"));
    let mut pair = input(Some(&ran), None, Timed::NotRun, &[]);
    assert_eq!(judge(&pair).status, Status::Unverified);
    pair.supported = false;
    assert_eq!(judge(&pair).status, Status::Unsupported);

    let crashed = Ran::<VerifyOutput>::Failed("exit 101".into());
    let judged = judge(&input(
        Some(&crashed),
        Some(&golden("same")),
        Timed::NotRun,
        &[],
    ));
    assert_eq!(
        (judged.status, judged.error.as_deref()),
        (Status::Failed, Some("exit 101"))
    );
    assert_eq!(
        judge(&input(None, None, Timed::NotRun, &[])).status,
        Status::Failed
    );
    let timed_out = judge(&input(Some(&ran), None, Timed::Failed("timed out"), &[]));
    assert_eq!(timed_out.status, Status::Failed);
}

#[test]
fn a_timed_hash_that_differs_from_verify_is_failed_even_without_a_golden() {
    let ran = Ran::Ok(output("same"));
    let hashes = [output("same").final_sha256, "different".to_string()];
    for g in [None, Some(golden("same"))] {
        let judged = judge(&input(Some(&ran), g.as_ref(), Timed::Hashes(&hashes), &[]));
        assert_eq!(judged.status, Status::Failed);
    }
}

#[test]
fn first_difference_reads_cursor_history_attrs_and_counts() {
    let base = dump_with("a");
    let mut moved = base.clone();
    moved.cursor = (1, 2);
    assert_eq!(
        first_difference(std::slice::from_ref(&base), &[moved]),
        "checkpoint 0, cursor: expected (0, 0) got (1, 2)"
    );
    let mut styled = base.clone();
    styled
        .attrs
        .push((0, 0, 1, "i1".into(), "d".into(), "b".into()));
    assert_eq!(
        first_difference(std::slice::from_ref(&base), &[styled]),
        "checkpoint 0, attrs: expected 0 runs, got 1, first extra (0, 0, 1, \"i1\", \"d\", \"b\")"
    );
    let mut longer = base.clone();
    longer.history.push("old".into());
    assert_eq!(
        first_difference(std::slice::from_ref(&base), &[longer]),
        "checkpoint 0, history: expected 0 rows, got 1"
    );
    assert_eq!(
        first_difference(std::slice::from_ref(&base), &[base.clone(), base.clone()]),
        "checkpoints: expected 1, got 2"
    );
}

#[test]
fn a_ghostty_style_scrollback_proof_fails_a_short_history_after_scrolling() {
    let mut short = output("same");
    short.scrollback_rows = Some(130);
    let ran = Ran::Ok(short);
    let expected = golden("same");
    let mut pair = input(Some(&ran), Some(&expected), Timed::NotRun, &[]);
    pair.fixture = "scrolling";
    let judged = judge(&pair);
    assert_eq!(judged.status, Status::Failed);
    assert!(judged.error.unwrap().contains("130 rows"));
    // Another fixture, or a long enough history, is not affected.
    pair.fixture = "plain_ascii";
    assert_eq!(judge(&pair).status, Status::Ok);
    let mut long = output("same");
    long.scrollback_rows = Some(10_040);
    let ran = Ran::Ok(long);
    let mut pair = input(Some(&ran), Some(&expected), Timed::NotRun, &[]);
    pair.fixture = "scrolling";
    assert_eq!(judge(&pair).status, Status::Ok);
}
