use super::*;
use crate::verify::verify_tests::output;

fn vote<'a>(engine: &str, output: &'a VerifyOutput) -> (String, &'a VerifyOutput) {
    (engine.to_string(), output)
}

#[test]
fn three_of_five_agreeing_make_the_golden() {
    let (same, other, odd) = (output("same"), output("other"), output("odd"));
    let votes = [
        vote("a", &same),
        vote("b", &other),
        vote("c", &same),
        vote("d", &odd),
        vote("e", &same),
    ];
    assert_eq!(
        decide(&votes),
        Decision::Golden {
            dumps: same.dumps.clone(),
            agreeing: vec!["a".into(), "c".into(), "e".into()]
        }
    );
}

#[test]
fn two_two_one_has_no_golden() {
    let (x, y, z) = (output("x"), output("y"), output("z"));
    let votes = [
        vote("a", &x),
        vote("b", &x),
        vote("c", &y),
        vote("d", &y),
        vote("e", &z),
    ];
    assert_eq!(
        decide(&votes),
        Decision::Ungated {
            group_sizes: vec![2, 2, 1]
        }
    );
}

#[test]
fn a_three_three_tie_picks_nobody() {
    let (x, y) = (output("x"), output("y"));
    let votes: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|e| vote(e, &x))
        .chain(["d", "e", "f"].iter().map(|e| vote(e, &y)))
        .collect();
    assert_eq!(
        decide(&votes),
        Decision::Ungated {
            group_sizes: vec![3, 3]
        }
    );
    assert_eq!(
        decide(&[]),
        Decision::Ungated {
            group_sizes: vec![]
        }
    );
}

#[test]
fn an_ungated_fixture_leaves_its_grid_rows_unverified() {
    use crate::verify::{PairInput, Ran, Status, Timed, judge};
    let ran = Ran::Ok(output("x"));
    let judged = judge(&PairInput {
        engine: "alacritty",
        fixture: "grapheme_clusters",
        supported: true,
        verify: Some(&ran),
        golden: None,
        timed: Timed::NotRun,
        accepted: &[],
    });
    assert_eq!(judged.status, Status::Unverified);
}

#[test]
fn accepted_and_ungated_files_parse_with_empty_defaults() {
    let accepted: AcceptedFile = toml::from_str("# nothing accepted yet\n").unwrap();
    assert!(accepted.accepted.is_empty());
    let one: AcceptedFile = toml::from_str(
        "[[accepted]]\nengine = \"vt100\"\nfixture = \"cjk_wide\"\nfinal_sha256 = \"ab\"\nreason = \"width\"\n",
    )
    .unwrap();
    assert_eq!(one.accepted[0].engine, "vt100");
    let ungated = UngatedFile {
        ungated: vec![UngatedEntry {
            fixture: "grapheme_clusters".into(),
            corpus_sha256: "cd".into(),
            group_sizes: vec![2, 2, 1],
        }],
    };
    let text = toml::to_string(&ungated).unwrap();
    assert_eq!(
        toml::from_str::<UngatedFile>(&text).unwrap().ungated,
        ungated.ungated
    );
}
