use std::cell::RefCell;

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("compare-machine-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn slugs_are_lower_case_digits_and_dashes() {
    for good in ["i7-12700-win11", "m1", "a-b-9"] {
        validate_slug(good).unwrap();
    }
    for bad in ["", "I7", "a b", "a_b", "a/b", "ü"] {
        assert!(validate_slug(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_second_run_the_same_day_gets_a_numeric_suffix() {
    let dir = scratch("stem");
    assert_eq!(output_stem(&dir, "2026-11-02"), "2026-11-02");
    fs::write(dir.join("2026-11-02.json"), "{}").unwrap();
    assert_eq!(output_stem(&dir, "2026-11-02"), "2026-11-02-2");
    fs::write(dir.join("2026-11-02-2.json"), "{}").unwrap();
    assert_eq!(output_stem(&dir, "2026-11-02"), "2026-11-02-3");
    assert_eq!(output_stem(&dir, "2026-11-03"), "2026-11-03");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn civil_dates_match_known_days() {
    assert_eq!(civil_date(0), "1970-01-01");
    assert_eq!(civil_date(19_723), "2024-01-01");
    assert_eq!(civil_date(20_512), "2026-02-28");
    assert_eq!(civil_date(20_513), "2026-03-01");
    assert_eq!(civil_date(11_016), "2000-02-29");
}

/// `(pid, parent, name)` triples as a process list; the driver is pid 100.
fn list(parts: &[(u32, u32, &str)]) -> Vec<ProcessEntry> {
    parts
        .iter()
        .map(|(pid, parent, name)| ProcessEntry {
            pid: *pid,
            parent: *parent,
            name: name.to_string(),
        })
        .collect()
}

#[test]
fn cargo_or_rustc_running_refuses_the_run() {
    let quiet = list(&[
        (1, 0, "explorer.exe"),
        (2, 1, "node.exe"),
        (3, 1, "cargo-fmt.exe"),
    ]);
    refuse_if_building(&quiet, 100).unwrap();
    let busy = list(&[
        (1, 0, "explorer.exe"),
        (2, 1, "cargo.exe"),
        (3, 1, "RUSTC.EXE"),
    ]);
    let error = refuse_if_building(&busy, 100).unwrap_err();
    let text = format!("{error:#}");
    assert!(
        text.contains("cargo.exe") && text.contains("RUSTC.EXE"),
        "{text}"
    );
    assert!(refuse_if_building(&list(&[(2, 1, "rustc")]), 100).is_err());
}

#[test]
fn the_cargo_that_started_the_driver_does_not_count_but_a_sibling_does() {
    // explorer -> shell -> cargo run -> compare (100)
    let under_cargo_run = list(&[
        (1, 0, "explorer.exe"),
        (10, 1, "bash.exe"),
        (20, 10, "cargo.exe"),
        (100, 20, "compare.exe"),
    ]);
    refuse_if_building(&under_cargo_run, 100).unwrap();
    // A second cargo elsewhere in the tree is a real build.
    let mut with_build = under_cargo_run.clone();
    with_build.push(list(&[(30, 10, "cargo.exe")]).remove(0));
    assert!(refuse_if_building(&with_build, 100).is_err());
    // A cargo that is a child of the driver is not an ancestor either.
    let child = list(&[
        (100, 20, "compare.exe"),
        (20, 0, "cargo.exe"),
        (40, 100, "rustc.exe"),
    ]);
    let text = format!("{:#}", refuse_if_building(&child, 100).unwrap_err());
    assert!(
        text.contains("rustc.exe") && !text.contains("cargo.exe"),
        "{text}"
    );
}

#[test]
fn an_ancestor_walk_survives_a_pid_cycle() {
    let cyclic = list(&[(100, 20, "compare.exe"), (20, 100, "cargo.exe")]);
    refuse_if_building(&cyclic, 100).unwrap();
}

#[test]
fn powercfg_output_parses_on_any_locale() {
    let english = "Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)\n";
    let scheme = parse_active_scheme(english).unwrap();
    assert_eq!(scheme.guid, "381b4222-f694-41f0-9685-ff5bb260df2e");
    assert_eq!(scheme.name, "Balanced");
    assert_eq!(
        scheme.label(),
        "Balanced 381b4222-f694-41f0-9685-ff5bb260df2e"
    );
    let other = "GUID sơ đồ nguồn: 8C5E7FDA-E8BF-4A96-9A85-A6E23A8C635C  (Hiệu suất cao)";
    let scheme = parse_active_scheme(other).unwrap();
    assert_eq!(scheme.guid, "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c");
    assert_eq!(scheme.name, "Hiệu suất cao");
    assert_eq!(parse_active_scheme("nothing here"), None);
}

/// Records every call; `fail_switch` makes the switch to High performance fail.
struct FakePower {
    active: RefCell<Scheme>,
    calls: RefCell<Vec<String>>,
    fail_switch: bool,
}

impl FakePower {
    fn balanced(fail_switch: bool) -> Self {
        Self {
            active: RefCell::new(Scheme {
                guid: "bal".into(),
                name: "Balanced".into(),
            }),
            calls: RefCell::new(Vec::new()),
            fail_switch,
        }
    }
}

impl PowerControl for FakePower {
    fn active(&self) -> Result<Scheme> {
        Ok(self.active.borrow().clone())
    }

    fn set_active(&self, target: &str) -> Result<()> {
        self.calls.borrow_mut().push(target.to_string());
        if self.fail_switch && target == HIGH_PERFORMANCE {
            bail!("access denied");
        }
        let (guid, name) = if target == HIGH_PERFORMANCE {
            ("high", "High performance")
        } else {
            (target, "restored")
        };
        *self.active.borrow_mut() = Scheme {
            guid: guid.into(),
            name: name.into(),
        };
        Ok(())
    }
}

#[test]
fn a_run_switches_to_high_performance_and_restores_on_ok() {
    let dir = scratch("ok");
    let file = dir.join("power-restore.txt");
    let power = FakePower::balanced(false);
    {
        let session = begin_power_session(&power, &file, false).unwrap();
        assert_eq!(session.before.guid, "bal");
        assert_eq!(session.during.name, "High performance");
        assert_eq!(fs::read_to_string(&file).unwrap(), "bal");
        assert!(session.guard.is_some());
    }
    assert_eq!(*power.calls.borrow(), [HIGH_PERFORMANCE, "bal"]);
    assert_eq!(power.active.borrow().guid, "bal");
    assert!(!file.exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_plan_is_restored_on_an_error_return_and_on_panic() {
    let dir = scratch("err");
    let file = dir.join("power-restore.txt");
    let power = FakePower::balanced(false);
    let failing = || -> Result<()> {
        let _session = begin_power_session(&power, &file, false)?;
        bail!("run failed")
    };
    assert!(failing().is_err());
    assert_eq!(power.active.borrow().guid, "bal");
    assert!(!file.exists());

    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _session = begin_power_session(&power, &file, false).unwrap();
        panic!("boom");
    }));
    assert!(panicked.is_err());
    assert_eq!(power.active.borrow().guid, "bal");
    assert!(!file.exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failed_switch_refuses_and_changes_nothing() {
    let dir = scratch("refuse");
    let file = dir.join("power-restore.txt");
    let power = FakePower::balanced(true);
    let error = begin_power_session(&power, &file, false).err().unwrap();
    assert!(format!("{error:#}").contains("High performance"));
    assert_eq!(power.active.borrow().guid, "bal");
    assert!(!file.exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_leftover_file_from_a_killed_run_is_restored_first() {
    let dir = scratch("leftover");
    let file = dir.join("power-restore.txt");
    fs::write(&file, "orig").unwrap();
    let power = FakePower::balanced(false);
    let session = begin_power_session(&power, &file, true).unwrap();
    assert_eq!(*power.calls.borrow(), ["orig"]);
    assert_eq!(session.before.guid, "orig");
    assert!(!file.exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn keep_power_plan_neither_switches_nor_restores() {
    let dir = scratch("keep");
    let file = dir.join("power-restore.txt");
    let power = FakePower::balanced(false);
    {
        let session = begin_power_session(&power, &file, true).unwrap();
        assert!(session.guard.is_none());
        assert_eq!(session.before, session.during);
    }
    assert!(power.calls.borrow().is_empty());
    assert!(!file.exists());
    fs::remove_dir_all(&dir).unwrap();
}
