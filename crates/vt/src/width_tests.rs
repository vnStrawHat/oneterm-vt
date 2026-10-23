use super::*;

/// `(table row, text, scalar_width if the text is one scalar, cluster_width)`.
const CASES: &[(u8, &str, Option<u8>, u8)] = &[
    (1, "a", Some(1), 1),
    (2, "\u{6F22}", Some(2), 2),
    (3, "\u{FF21}", Some(2), 2),
    (4, "\u{3000}", Some(2), 2),
    (5, "\u{B1}", Some(1), 1),
    (6, "\u{E000}", Some(1), 1),
    (7, "\u{0}", Some(0), 0),
    (7, "\u{7}", Some(0), 0),
    (7, "\u{1B}", Some(0), 0),
    (7, "\u{7F}", Some(0), 0),
    (7, "\u{9B}", Some(0), 0),
    (8, "\r\n", None, 0),
    (9, "\u{301}", Some(0), 0),
    (10, "e\u{301}", None, 1),
    (11, "\u{200D}", Some(0), 0),
    (12, "\u{FE0F}", Some(0), 0),
    (13, "\u{FE0E}", Some(0), 0),
    (14, "\u{2764}", Some(1), 1),
    (14, "\u{2764}\u{FE0F}", None, 2),
    (15, "\u{231A}", Some(2), 2),
    (15, "\u{231A}\u{FE0E}", None, 1),
    (16, "1\u{FE0F}", None, 2),
    (16, "#\u{FE0F}\u{20E3}", None, 2),
    (17, "a\u{FE0F}", None, 1),
    (18, "\u{6F22}\u{FE0E}", None, 2),
    (19, "\u{1F642}", Some(2), 2),
    (20, "\u{1F44D}\u{1F3FD}", None, 2),
    (21, "\u{1F3FD}", Some(2), 2),
    (
        22,
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}",
        None,
        2,
    ),
    (23, "\u{1F468}\u{200D}\u{1F4BB}", None, 2),
    (24, "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}", None, 2),
    (25, "\u{1F1FA}\u{1F1F8}", None, 2),
    (26, "\u{1100}", Some(2), 2),
    (26, "\u{1161}", Some(0), 0),
    (26, "\u{11A8}", Some(0), 0),
    (27, "\u{1100}\u{1161}\u{11A8}", None, 2),
    (28, "\u{AC00}", Some(2), 2),
    (29, "\u{17D8}", Some(2), 2),
    (30, "ab", None, 1),
    (30, "", None, 0),
];

#[test]
fn width_cases() {
    for &(row, text, scalar, cluster) in CASES {
        if let Some(expected) = scalar {
            let mut chars = text.chars();
            let c = chars.next().unwrap();
            assert!(
                chars.next().is_none(),
                "row {row}: {text:?} is not one scalar"
            );
            assert_eq!(
                scalar_width(c),
                expected,
                "row {row}: scalar_width({text:?})"
            );
        }
        assert_eq!(
            cluster_width(text),
            cluster,
            "row {row}: cluster_width({text:?})"
        );
    }
}

#[test]
fn zwj_family_per_scalar_sum_is_8() {
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
    let sum: u32 = family.chars().map(|c| u32::from(scalar_width(c))).sum();
    assert_eq!(sum, 8);
}

/// Scalars where `cluster_width` of the lone scalar differs from `scalar_width`.
const EXCEPTIONS: &[char] = &[];

#[test]
fn every_scalar_value() {
    let mut buf = [0u8; 4];
    for c in '\0'..=char::MAX {
        let single = scalar_width(c);
        assert!(single <= 2, "scalar_width({:#X}) = {single}", u32::from(c));
        if c.is_control() {
            assert_eq!(single, 0, "control {:#X}", u32::from(c));
        } else if !EXCEPTIONS.contains(&c) {
            assert_eq!(
                cluster_width(c.encode_utf8(&mut buf)),
                single,
                "cluster_width vs scalar_width for {:#X}",
                u32::from(c)
            );
        }
    }
}
