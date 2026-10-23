use super::*;
use crate::app_inspection::identity::fixture;
use std::{cell::Cell, fs, os::unix::fs::symlink};

const ID: &str = "io.moomole.native.fixture";

struct Fixture {
    temp: tempfile::TempDir,
    home: PathBuf,
    app: PathBuf,
    snapshot: identity::Snapshot,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("MooMole-bundle-target-")
            .tempdir()
            .unwrap();
        let home = temp.path().canonicalize().unwrap();
        let app = fixture(
            &home,
            &format!("MooMole-{}-中文 '#%*.app", uuid::Uuid::new_v4()),
            ID,
        );
        fs::create_dir_all(app.join("Contents/Frameworks/F.framework/Versions/A/Resources"))
            .unwrap();
        fs::write(
            app.join("Contents/Frameworks/F.framework/Versions/A/Resources/data"),
            "bundled original",
        )
        .unwrap();
        symlink(
            "A",
            app.join("Contents/Frameworks/F.framework/Versions/Current"),
        )
        .unwrap();
        fs::write(home.join("personal-retained"), "outside retained").unwrap();
        symlink(
            home.join("personal-retained"),
            app.join("Contents/outside-link"),
        )
        .unwrap();
        let snapshot = identity::capture(&app, ID).unwrap();
        Self {
            temp,
            home,
            app,
            snapshot,
        }
    }
    fn prepare(&self) -> PreparedBundle {
        let now = crate::db::now();
        PreparedBundle::prepare_with(&self.app, &self.snapshot, now, now + 300_000, |_| {}).unwrap()
    }
    fn resource(&self) -> PathBuf {
        self.app
            .join("Contents/Frameworks/F.framework/Versions/A/Resources/data")
    }
    fn replace(&self, parent: bool) -> PathBuf {
        let relocated = if parent {
            let to = self.home.join("previous-applications");
            fs::rename(self.app.parent().unwrap(), &to).unwrap();
            to.join(self.app.file_name().unwrap())
        } else {
            let to = self.home.join("relocated-original.app");
            fs::rename(&self.app, &to).unwrap();
            to
        };
        assert_eq!(
            fixture(
                &self.home,
                self.app.file_name().unwrap().to_str().unwrap(),
                ID
            ),
            self.app
        );
        fs::write(
            self.app.join("Contents/replacement-only"),
            "unselected replacement",
        )
        .unwrap();
        relocated
    }
    fn assert_outside(&self) {
        assert_eq!(
            fs::read_to_string(self.home.join("personal-retained")).unwrap(),
            "outside retained"
        );
    }
}

#[test]
fn native_bundle_preparation_is_bound_to_the_independent_plan_and_read_only() {
    autoreleasepool(|_| {
        let f = Fixture::new();
        let ctx = Context {
            home: f.home.clone(),
            engine_root: Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mole"),
            current_app: None,
            current_id: "io.github.rememberber.moomole".into(),
            protected_paths: vec![],
        };
        let row =
            serde_json::json!({"path":f.app,"bundle_id":ID,"name":"Native fixture","source":"App"});
        let source =
            crate::uninstall_plan::Source::from_row(&uuid::Uuid::new_v4().to_string(), 0, &row)
                .unwrap();
        let plan = Plan::prepare(source, &ctx).unwrap();
        let prepared = PreparedBundle::prepare(&plan, &ctx).unwrap();
        prepared.validate().unwrap();
        assert!(prepared.reference.isFileReferenceURL());
        assert_eq!(identity::capture(&f.app, ID).unwrap(), f.snapshot);
        assert_eq!(
            serde_json::to_value(plan.view(crate::db::now())).unwrap()["canExecute"],
            false
        );
        let mut protected = ctx.clone();
        protected
            .protected_paths
            .push(f.app.to_str().unwrap().into());
        assert!(PreparedBundle::prepare(&plan, &protected).is_err());
        f.assert_outside();
    });
}

#[test]
fn bundle_replacement_during_native_reference_acquisition_is_refused() {
    autoreleasepool(|_| {
        for phase in [Preparation::Anchored, Preparation::Referenced] {
            let f = Fixture::new();
            let now = crate::db::now();
            let mut moved = None;
            let result =
                PreparedBundle::prepare_with(&f.app, &f.snapshot, now, now + 300_000, |at| {
                    if phase == at {
                        moved = Some(f.replace(false));
                    }
                });
            assert!(result.is_err());
            assert_eq!(
                fs::metadata(moved.unwrap()).unwrap().ino(),
                f.snapshot.root.nodes.last().unwrap().ino
            );
            assert!(f.app.join("Contents/replacement-only").is_file());
            f.assert_outside();
        }
    });
}

#[test]
fn changed_bundle_parent_deep_content_or_links_never_reach_the_native_api() {
    autoreleasepool(|_| {
        for mode in ["name", "parent", "deepContent", "link", "plist", "main"] {
            let f = Fixture::new();
            let prepared = f.prepare();
            match mode {
                "name" => {
                    f.replace(false);
                }
                "parent" => {
                    f.replace(true);
                }
                "deepContent" => {
                    fs::write(f.resource(), "changed descendant after preparation").unwrap()
                }
                "link" => {
                    fs::remove_file(f.app.join("Contents/outside-link")).unwrap();
                    symlink("different-target", f.app.join("Contents/outside-link")).unwrap();
                }
                "plist" => fs::write(f.app.join("Contents/Info.plist"), "changed plist").unwrap(),
                "main" => fs::write(
                    f.app.join("Contents/MacOS/fixture-main"),
                    "changed executable",
                )
                .unwrap(),
                _ => unreachable!(),
            }
            let called = Cell::new(false);
            let gate = Cell::new(false);
            let result = prepared.trash_with(
                || {
                    gate.set(true);
                    Ok(())
                },
                |_| {
                    called.set(true);
                    NativeResult {
                        succeeded: true,
                        destination: None,
                    }
                },
            );
            assert!(result.is_err(), "{mode}");
            assert!(!called.get() && !gate.get(), "{mode}");
            f.assert_outside();
        }
    });
}

#[test]
fn expiry_account_permissions_and_final_gate_refusals_never_call_native() {
    autoreleasepool(|_| {
        for mode in ["expiry", "rollback", "account", "permissions", "gate"] {
            let f = Fixture::new();
            let mut prepared = f.prepare();
            match mode {
                "expiry" => prepared.expires_at = crate::db::now() - 1,
                "rollback" => prepared.created_at = crate::db::now() + 300_000,
                "account" => prepared.uid = prepared.uid.saturating_add(1),
                "permissions" => {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(f.app.parent().unwrap(), fs::Permissions::from_mode(0o500))
                        .unwrap();
                }
                _ => {}
            }
            let called = Cell::new(false);
            let result = prepared.trash_with(
                || Err("sealed execution gate refused".into()),
                |_| {
                    called.set(true);
                    NativeResult {
                        succeeded: true,
                        destination: None,
                    }
                },
            );
            if mode == "permissions" {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(f.app.parent().unwrap(), fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
            assert!(result.is_err(), "{mode}");
            assert!(!called.get());
            assert!(f.app.is_dir());
            f.assert_outside();
        }
    });
}

#[test]
fn post_call_results_require_actual_destination_original_package_and_source_scope() {
    autoreleasepool(|_| {
        for mode in [
            "stable",
            "sourceReplaced",
            "deepChanged",
            "wrongBundle",
            "nativeError",
            "noDestination",
            "noChange",
            "wrongType",
        ] {
            let f = Fixture::new();
            let prepared = f.prepare();
            let destination = f.home.join("owned-recovery.app");
            let attempt = prepared
                .trash_with(
                    || Ok(()),
                    |_| {
                        if mode != "noChange" {
                            fs::rename(&f.app, &destination).unwrap();
                        }
                        match mode {
                            "sourceReplaced" => {
                                fixture(&f.home, f.app.file_name().unwrap().to_str().unwrap(), ID);
                            }
                            "deepChanged" => fs::write(
                                destination.join(
                                    "Contents/Frameworks/F.framework/Versions/A/Resources/data",
                                ),
                                "late deep change",
                            )
                            .unwrap(),
                            "wrongBundle" => {
                                let other = fixture(&f.home, "Other.app", ID);
                                fs::rename(&destination, f.home.join("original-retained.app"))
                                    .unwrap();
                                fs::rename(other, &destination).unwrap();
                            }
                            "wrongType" => {
                                fs::rename(&destination, f.home.join("original-retained.app"))
                                    .unwrap();
                                symlink(f.home.join("original-retained.app"), &destination)
                                    .unwrap();
                            }
                            _ => {}
                        }
                        NativeResult {
                            succeeded: mode != "nativeError",
                            destination: if matches!(mode, "noDestination" | "noChange") {
                                None
                            } else {
                                Some(destination.clone())
                            },
                        }
                    },
                )
                .unwrap();
            if mode == "stable" {
                let Outcome::Moved(receipt) = attempt.outcome else {
                    panic!("{}", attempt.reason)
                };
                assert_eq!(
                    receipt.observed_inventory,
                    Some(f.snapshot.inventory.clone())
                );
                assert_eq!(
                    receipt.actual.inode,
                    f.snapshot.root.nodes.last().unwrap().ino
                );
                let value = serde_json::to_value(receipt).unwrap();
                assert!(value["actual"]["device"].is_string());
                assert!(value["actual"]["inode"].is_string());
            } else {
                assert!(matches!(attempt.outcome, Outcome::Uncertain(_)), "{mode}");
            }
            f.assert_outside();
        }
    });
}

#[test]
fn relocation_allows_only_root_ctime_change_and_preserves_descendant_evidence() {
    let f = Fixture::new();
    let destination = f.home.join("owned-recovery.app");
    fs::rename(&f.app, &destination).unwrap();
    let after = identity::capture(&destination, ID).unwrap();
    assert!(relocated_matches(&f.snapshot, &after, &f.app, &destination));
    let mut root_time = after.clone();
    root_time.root.ctime += 1;
    assert!(relocated_matches(
        &f.snapshot,
        &root_time,
        &f.app,
        &destination
    ));
    for mode in [
        "rootIdentity",
        "infoHash",
        "mainTime",
        "contentsTime",
        "inventory",
        "bundleId",
    ] {
        let mut changed = after.clone();
        match mode {
            "rootIdentity" => changed.root.nodes.last_mut().unwrap().ino += 1,
            "infoHash" => changed.info_sha256 = "0".repeat(64),
            "mainTime" => changed.executable.ctime_ns += 1,
            "contentsTime" => changed.contents.ctime_ns += 1,
            "inventory" => changed.inventory.digest = "0".repeat(64),
            "bundleId" => changed.bundle_id = "another.id".into(),
            _ => unreachable!(),
        }
        assert!(
            !relocated_matches(&f.snapshot, &changed, &f.app, &destination),
            "{mode}"
        );
    }
}

fn persist_report(path: &Path, report: &serde_json::Value) {
    use std::io::Write;
    let mut file = fs::File::create(path).unwrap();
    file.write_all(&serde_json::to_vec_pretty(report).unwrap())
        .unwrap();
    file.sync_all().unwrap();
    fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}

#[test]
#[ignore = "Explicit rescue of this test's own saved native fixture; requires MOOMOLE_BUNDLE_RESCUE_REPORT, exact original identity and an absent source"]
fn rescue_owned_native_bundle_fixture() {
    autoreleasepool(|_| {
        let report_path = PathBuf::from(
            std::env::var_os("MOOMOLE_BUNDLE_RESCUE_REPORT")
                .expect("explicit owned fixture report required"),
        );
        let home = report_path.parent().unwrap().canonicalize().unwrap();
        assert_eq!(
            home.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(home
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("MooMole-bundle-target-"));
        assert_eq!(
            report_path.file_name().unwrap(),
            "native-bundle-rescue.json"
        );
        assert_eq!(fs::metadata(&home).unwrap().uid(), unsafe {
            libc::geteuid()
        });
        let mut report: serde_json::Value =
            serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(
            report["fixtureRoot"].as_str().unwrap(),
            home.to_str().unwrap()
        );
        assert_eq!(report["restored"], false);
        let source = PathBuf::from(report["originalLocation"].as_str().unwrap());
        assert_eq!(source.parent(), Some(home.join("Applications").as_path()));
        assert_eq!(source, PathBuf::from(report["source"].as_str().unwrap()));
        assert!(
            fs::symlink_metadata(&source).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        );
        let receipt: crate::execution::Receipt =
            serde_json::from_value(report["receipt"]["actual"].clone()).unwrap();
        let actual = PathBuf::from(&receipt.destination);
        assert_eq!(actual.file_name(), source.file_name());
        let current = identity::capture_returned(&actual, ID).unwrap();
        let original = report["snapshot"]["root"]["nodes"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        let node = current.root.nodes.last().unwrap();
        assert_eq!(
            (node.dev, node.ino),
            (
                original["dev"].as_u64().unwrap(),
                original["ino"].as_u64().unwrap()
            )
        );
        assert_eq!((node.dev, node.ino), (receipt.device, receipt.inode));
        assert_eq!(
            serde_json::to_value(&current.inventory).unwrap(),
            report["snapshot"]["inventory"]
        );
        assert_eq!(
            current.info_sha256,
            report["snapshot"]["infoSha256"].as_str().unwrap()
        );
        NSFileManager::defaultManager()
            .moveItemAtURL_toURL_error(
                &NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination)),
                &NSURL::fileURLWithPath(&NSString::from_str(source.to_str().unwrap())),
            )
            .unwrap();
        let restored = identity::capture(&source, ID).unwrap();
        assert!(relocated_matches(&current, &restored, &actual, &source));
        assert!(
            fs::symlink_metadata(&actual).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        );
        assert_eq!(
            fs::read_to_string(home.join("personal-retained")).unwrap(),
            "outside retained"
        );
        report["restored"] = true.into();
        report["rescueVerified"] = true.into();
        persist_report(&report_path, &report);
        eprintln!(
            "Exact original owned App restored; saved rescue report retained at {}",
            report_path.display()
        );
    });
}

#[test]
#[ignore = "Explicit native Trash experiment: fresh owned App bundles only, saves rescue evidence and restores exact returned item without overwrite"]
fn native_whole_app_trash_receipt_and_remaining_race_window() {
    autoreleasepool(|_| {
        for mode in ["stable", "beforeGate", "afterGateName", "afterGateDeep"] {
            let mut f = Fixture::new();
            // Preserve source and rescue evidence if any assertion/OS call fails.
            f.temp.disable_cleanup(true);
            let report_path = f.home.join("native-bundle-rescue.json");
            let mut report = serde_json::json!({"fixtureRoot":f.home,"source":f.app,"mode":mode,"state":"prepared","snapshot":f.snapshot,"restored":false});
            persist_report(&report_path, &report);
            eprintln!("Native whole-App fixture {mode}: {}", f.home.display());
            let prepared = f.prepare();
            let mut original_location = f.app.clone();
            if mode == "beforeGate" {
                original_location = f.replace(false);
            }
            let result = prepared.move_to_trash_checked(|| {
                if mode == "afterGateName" {
                    original_location = f.replace(false);
                }
                if mode == "afterGateDeep" {
                    fs::write(f.resource(), "changed after final bundle check").unwrap();
                }
                Ok(())
            });
            if mode == "beforeGate" {
                assert!(result.is_err());
                assert!(original_location.is_dir());
                report["state"] = "rejectedBeforeCall".into();
                persist_report(&report_path, &report);
                f.assert_outside();
                f.temp.disable_cleanup(false);
                continue;
            }
            let attempt = result.unwrap();
            let (state, receipt) = match attempt.outcome {
                Outcome::Moved(receipt) => ("moved", Some(receipt)),
                Outcome::Uncertain(receipt) => ("uncertain", receipt),
            };
            report["state"] = state.into();
            report["receipt"] = serde_json::to_value(&receipt).unwrap();
            report["originalLocation"] = serde_json::to_value(&original_location).unwrap();
            persist_report(&report_path, &report);
            let receipt = receipt.expect("native exact URL required; rescue evidence retained");
            let returned = Path::new(&receipt.actual.destination);
            let current = identity::capture_returned(returned, ID).unwrap();
            let node = current.root.nodes.last().unwrap();
            assert_eq!(
                (node.dev, node.ino),
                (receipt.actual.device, receipt.actual.inode)
            );
            // The system must have moved our original object reference, never
            // the unselected replacement. Evidence stays on disk on mismatch.
            assert_eq!(
                (node.dev, node.ino),
                (
                    f.snapshot.root.nodes.last().unwrap().dev,
                    f.snapshot.root.nodes.last().unwrap().ino
                )
            );
            assert!(original_location.starts_with(&f.home));
            assert!(fs::symlink_metadata(&original_location)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound));
            let from = NSURL::fileURLWithPath(&NSString::from_str(&receipt.actual.destination));
            let to =
                NSURL::fileURLWithPath(&NSString::from_str(original_location.to_str().unwrap()));
            NSFileManager::defaultManager()
                .moveItemAtURL_toURL_error(&from, &to)
                .unwrap();
            let restored = identity::capture(&original_location, ID).unwrap();
            assert!(relocated_matches(
                &current,
                &restored,
                returned,
                &original_location
            ));
            assert!(fs::symlink_metadata(returned)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound));
            report["restored"] = true.into();
            persist_report(&report_path, &report);
            assert_eq!(
                state,
                if mode == "stable" {
                    "moved"
                } else {
                    "uncertain"
                }
            );
            if mode == "afterGateName" {
                assert!(f.app.join("Contents/replacement-only").is_file());
            }
            f.assert_outside();
            eprintln!("Native whole-App {mode}: {state}, exact owned bundle restored, outside data retained.");
            f.temp.disable_cleanup(false);
        }
    });
}
