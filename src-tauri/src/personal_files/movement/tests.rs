use super::*;
use crate::engine::{Job, TaskKind};
use std::sync::Arc;

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    source: PathBuf,
    destination: PathBuf,
    settings: db::Settings,
    items: Vec<Item>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("MooMole-move-owned-")
            .tempdir()
            .unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("Downloads");
        let destination = root.join("Organized");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("chosen.txt"), "owned unique file").unwrap();
        let scan =
            super::super::scan::collect(&source, &[], Arc::new(Job::new(TaskKind::PersonalScan)))
                .unwrap();
        Self {
            _temp: temp,
            root,
            source,
            destination,
            settings: db::Settings {
                acknowledged: true,
                ..Default::default()
            },
            items: scan.items,
        }
    }
    fn plan(&self, conflict: Conflict) -> Movement {
        prepare(
            &self.items,
            self.destination.to_str().unwrap(),
            conflict,
            &self.settings,
        )
        .unwrap()
    }
    fn target(&self, m: &Movement) -> String {
        m.targets[0].destination.clone().unwrap()
    }
    fn prepared(&self, m: &Movement) -> Prepared {
        Prepared::new(&self.items[0], m, &self.target(m), &self.settings).unwrap()
    }
}
#[test]
fn collision_plans_are_explicit_bounded_and_do_not_mutate_files() {
    let f = Fixture::new();
    std::fs::write(f.destination.join("chosen.txt"), "existing").unwrap();
    assert!(f.plan(Conflict::Skip).targets[0].destination.is_none());
    let both = f.plan(Conflict::KeepBoth);
    assert_eq!(
        Path::new(&f.target(&both)).file_name().unwrap(),
        "chosen (1).txt"
    );
    let other = f.root.join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("chosen.txt"), "other unique").unwrap();
    let mut items = f.items.clone();
    let mut second = f.items[0].clone();
    second.id = uuid::Uuid::new_v4().to_string();
    second.path = other.join("chosen.txt").to_str().unwrap().into();
    second.witness = crate::observation::witness(Path::new(&second.path)).unwrap();
    items.push(second);
    let two = prepare(
        &items,
        f.destination.to_str().unwrap(),
        Conflict::KeepBoth,
        &f.settings,
    )
    .unwrap();
    assert!(two.targets[1]
        .destination
        .as_ref()
        .unwrap()
        .ends_with("chosen (2).txt"));
    assert_eq!(
        std::fs::read_to_string(f.destination.join("chosen.txt")).unwrap(),
        "existing"
    );
    assert_eq!(
        std::fs::read_to_string(f.source.join("chosen.txt")).unwrap(),
        "owned unique file"
    );
    let same = prepare(
        &f.items,
        f.source.to_str().unwrap(),
        Conflict::KeepBoth,
        &f.settings,
    )
    .unwrap();
    assert!(same.targets[0].destination.is_none());
}
#[test]
fn native_move_preserves_identity_and_late_collision_never_overwrites() {
    for collision in [false, true] {
        let f = Fixture::new();
        let movement = f.plan(Conflict::KeepBoth);
        if collision {
            std::fs::write(f.target(&movement), "late arrival").unwrap();
        }
        let prepared = f.prepared(&movement);
        let (outcome, _) = prepared.move_checked(|| Ok(())).unwrap();
        if collision {
            assert!(matches!(outcome, Outcome::Failed));
            assert_eq!(
                std::fs::read_to_string(f.target(&movement)).unwrap(),
                "late arrival"
            );
            assert!(f.source.join("chosen.txt").exists());
        } else {
            let Outcome::Moved(receipt) = outcome else {
                panic!("move did not produce a verified receipt")
            };
            assert_eq!(receipt.destination, f.target(&movement));
            let m = std::fs::metadata(&receipt.destination).unwrap();
            assert_eq!((m.dev(), m.ino()), (receipt.device, receipt.inode));
            assert_eq!(
                std::fs::read_to_string(receipt.destination).unwrap(),
                "owned unique file"
            );
            assert!(!f.source.join("chosen.txt").exists());
        }
    }
}
#[test]
fn changed_directory_file_protection_and_final_cancel_retain_the_source() {
    for mode in ["directory", "file", "protection", "cancel", "link"] {
        let f = Fixture::new();
        let movement = f.plan(Conflict::Skip);
        let prepared = f.prepared(&movement);
        match mode {
            "directory" => {
                std::fs::rename(&f.destination, f.root.join("old-target")).unwrap();
                std::fs::create_dir(&f.destination).unwrap();
            }
            "file" => {
                std::fs::write(&f.items[0].path, "changed content").unwrap();
            }
            "link" => {
                std::os::unix::fs::symlink(&f.source, f.destination.join("chosen.txt")).unwrap();
            }
            _ => {}
        }
        let result = prepared.move_checked(|| {
            if mode == "protection" || mode == "cancel" {
                return Err("retained by final gate".into());
            }
            Ok(())
        });
        assert!(result.is_err() || matches!(result.unwrap().0, Outcome::Failed));
        assert!(f.source.join("chosen.txt").exists(), "{mode}");
    }
    let f = Fixture::new();
    let mut settings = f.settings.clone();
    settings
        .protected_paths
        .push(f.destination.to_str().unwrap().into());
    assert!(prepare(
        &f.items,
        f.destination.to_str().unwrap(),
        Conflict::Skip,
        &settings
    )
    .is_err());
    let link = f.root.join("target-alias");
    std::os::unix::fs::symlink(&f.destination, &link).unwrap();
    assert!(prepare(
        &f.items,
        link.to_str().unwrap(),
        Conflict::Skip,
        &f.settings
    )
    .is_err());
}
