//! A complete, TEST-ONLY selected-file workflow against the real SQLite journal.
//! Every source is a direct child of Fixture's fresh private source directory.
//! Rule decisions are explicitly fixtures; they never authorize personal data.
//! The ordinary backend uses a private recovery folder, NOT the system Trash.
//! An ignored Foundation integration check uses ONLY these same fresh fixtures.
use super::Fixture;
use crate::{db, execution as journal, observation::AnchoredPath, permissions, review};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::CString,
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
};

const CONFIRMATION_LIFETIME: i64 = 30_000;

struct Confirmation {
    plan_id: String,
    digest: String,
    confirmed_at: i64,
    expires_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    BeforeCheck,
    BeforeIntent,
    AfterIntent,
    AfterMove,
    AfterResult,
}

#[derive(Clone, Copy)]
enum FixtureRule {
    PassedFixture,
    RetainFixture,
    UnknownFixture,
}

enum Backend {
    PrivateRecovery,
    NativeTrash,
}

struct Harness {
    fixture: Fixture,
    cancelled: bool,
    rules: HashMap<String, FixtureRule>,
    backend: Backend,
}

fn digest(plan: &review::Plan) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(plan).unwrap()))
}

impl Harness {
    fn new() -> Self {
        Self {
            fixture: Fixture::new(),
            cancelled: false,
            // No rule entry means unknown, including newly added items.
            rules: HashMap::new(),
            backend: Backend::PrivateRecovery,
        }
    }

    fn permit_fixture_rules(&mut self) {
        for item in &self.fixture.plan.items {
            self.rules
                .insert(item.candidate.id.clone(), FixtureRule::PassedFixture);
        }
    }

    fn confirm(&self) -> Confirmation {
        let plan = db::plan(&self.fixture.db, &self.fixture.plan.id).unwrap();
        Confirmation {
            plan_id: plan.id.clone(),
            digest: digest(&plan),
            confirmed_at: self.fixture.at,
            expires_at: plan.expires_at.min(self.fixture.at + CONFIRMATION_LIFETIME),
        }
    }

    fn run(
        &mut self,
        confirmation: &Confirmation,
        mut hook: impl FnMut(Phase, usize, &mut Self) -> Result<(), String>,
    ) -> Result<String, String> {
        // Only an ID crosses the boundary. Paths and selected IDs are loaded
        // from SQLite, then bound to the exact snapshot the fixture confirmed.
        let plan = db::plan(&self.fixture.db, &confirmation.plan_id)?;
        if digest(&plan) != confirmation.digest
            || self.fixture.at < confirmation.confirmed_at
            || self.fixture.at >= confirmation.expires_at
        {
            return Err("confirmation changed or expired".into());
        }
        // This harness has NO way to authorize arbitrary real cleanup roots.
        let source = self.fixture.root.join("source");
        if plan
            .items
            .iter()
            .any(|item| Path::new(&item.candidate.path).parent() != Some(source.as_path()))
        {
            return Err("outside this fresh fixture's source".into());
        }
        let run = journal::begin(&mut self.fixture.db, &plan, self.fixture.at)?;
        for (index, item) in plan.items.iter().enumerate() {
            hook(Phase::BeforeCheck, index, self)?;
            if self.cancelled {
                journal::finish(&mut self.fixture.db, &run, true, self.fixture.at)?;
                return Ok(run);
            }
            let id = &item.candidate.id;
            // Read protection anew FOR EACH item, never from the dialog or an
            // earlier preflight. A failed read aborts the driver before intent.
            let protected = db::settings(&self.fixture.db)?.protected_paths;
            let one = review::Plan {
                items: vec![item.clone()],
                ..plan.clone()
            };
            let check = review::check(&one, &protected, self.fixture.at);
            let current = &check.items[0];
            let rejection = if self.fixture.at < confirmation.confirmed_at
                || self.fixture.at >= confirmation.expires_at
            {
                Some("confirmation expired during this batch")
            } else if current.state != "unchanged" {
                Some(current.reason.as_str())
            } else if current.permissions.code != permissions::Code::NoRestrictionObserved {
                Some("fixture permission observation incomplete or restricted")
            } else {
                match self.rules.get(id) {
                    Some(FixtureRule::PassedFixture) => None,
                    Some(FixtureRule::RetainFixture) => Some("fixture rule requires retention"),
                    Some(FixtureRule::UnknownFixture) | None => Some("fixture rule unknown"),
                }
            };
            if let Some(why) = rejection {
                journal::skip(&mut self.fixture.db, &run, id, why, self.fixture.at)?;
                continue;
            }
            let held = match AnchoredPath::open(Path::new(&item.candidate.path)) {
                Ok(held)
                    if Some(&held.witness) == item.witness.as_ref()
                        && held.witness.nodes.last().unwrap().mode & libc::S_IFMT as u32
                            == libc::S_IFREG as u32 =>
                {
                    held
                }
                _ => {
                    journal::skip(
                        &mut self.fixture.db,
                        &run,
                        id,
                        "fixture requires an unchanged single-link regular file",
                        self.fixture.at,
                    )?;
                    continue;
                }
            };
            // Acquire and bind the native reference BEFORE intent, while the
            // selected file is still verified. Never recreate it from a path
            // that might have been replaced after confirmation/intent.
            let native = if matches!(self.backend, Backend::NativeTrash) {
                match crate::cleanup_target::PreparedFile::prepare(item) {
                    Ok(target) => Some(target),
                    Err(reason) => {
                        journal::skip(&mut self.fixture.db, &run, id, &reason, self.fixture.at)?;
                        continue;
                    }
                }
            } else {
                None
            };
            hook(Phase::BeforeIntent, index, self)?;
            if self.cancelled {
                journal::finish(&mut self.fixture.db, &run, true, self.fixture.at)?;
                return Ok(run);
            }
            // Repeat after the potentially slow checks. Still not an atomic
            // binding: a deliberate later name race is covered below.
            let protected = db::settings(&self.fixture.db)?.protected_paths;
            let final_check = review::check(&one, &protected, self.fixture.at);
            if self.fixture.at < confirmation.confirmed_at
                || self.fixture.at >= confirmation.expires_at
                || final_check.items[0].state != "unchanged"
                || final_check.items[0].permissions.code != permissions::Code::NoRestrictionObserved
                || !matches!(self.rules.get(id), Some(FixtureRule::PassedFixture))
                || held.validate().is_err()
            {
                journal::skip(
                    &mut self.fixture.db,
                    &run,
                    id,
                    "final fixture conditions changed; retained",
                    self.fixture.at,
                )?;
                continue;
            }
            // Commit must succeed BEFORE calling any mutating filesystem API.
            journal::intent(&mut self.fixture.db, &run, id, self.fixture.at)?;
            hook(Phase::AfterIntent, index, self)?;
            // Once intent is committed this item is in flight. Cancellation
            // stops subsequent items; it never discards this item's result.
            let (outcome, why) = self.apply(&held, &item.candidate.path, native.as_ref())?;
            hook(Phase::AfterMove, index, self)?;
            let uncertain = matches!(outcome, journal::Outcome::Uncertain(_));
            journal::complete_item(
                &mut self.fixture.db,
                &run,
                id,
                outcome,
                &why,
                self.fixture.at,
            )?;
            hook(Phase::AfterResult, index, self)?;
            if uncertain {
                return Ok(run); // complete_item stopped all remaining items.
            }
        }
        journal::finish(&mut self.fixture.db, &run, self.cancelled, self.fixture.at)?;
        Ok(run)
    }

    fn apply(
        &self,
        held: &AnchoredPath,
        original: &str,
        native: Option<&crate::cleanup_target::PreparedFile>,
    ) -> Result<(journal::Outcome, String), String> {
        let destination = match self.backend {
            Backend::PrivateRecovery => {
                let target = self
                    .fixture
                    .root
                    .join("recovery")
                    .join(Path::new(original).file_name().unwrap());
                let directory = File::open(target.parent().unwrap()).map_err(|e| e.to_string())?;
                let name =
                    CString::new(Path::new(original).file_name().unwrap().as_encoded_bytes())
                        .map_err(|e| e.to_string())?;
                // Fixture roots and held parents only. RENAME_EXCL never
                // overwrites a recovery item; it does NOT prevent source races.
                let result = unsafe {
                    libc::renameatx_np(
                        held.parent().as_raw_fd(),
                        name.as_ptr(),
                        directory.as_raw_fd(),
                        name.as_ptr(),
                        libc::RENAME_EXCL,
                    )
                };
                if result != 0 {
                    return Ok((journal::Outcome::Failed, "fixture move refused".into()));
                }
                target
            }
            Backend::NativeTrash => {
                return match native
                    .ok_or("native fixture has no pre-intent reference")?
                    .trash_fixture(&self.fixture.root, || {})
                {
                    Ok(attempt) => Ok((attempt.outcome, attempt.reason)),
                    // validate failed before invoking the native API; this is a
                    // known no-change result, despite an already committed intent.
                    Err(reason) => Ok((
                        journal::Outcome::Failed,
                        format!("native API not called: {reason}"),
                    )),
                };
            }
        };
        let Ok(meta) = std::fs::symlink_metadata(&destination) else {
            return Ok((
                journal::Outcome::Uncertain(None),
                "fixture destination could not be observed".into(),
            ));
        };
        let before = held.witness.nodes.last().unwrap();
        let receipt = journal::Receipt {
            destination: destination.to_str().unwrap().into(),
            device: meta.dev(),
            inode: meta.ino(),
        };
        let matches = meta.is_file()
            && meta.dev() == before.dev
            && meta.ino() == before.ino
            && meta.mode() == before.mode
            && meta.uid() == before.uid
            && meta.gid() == before.gid
            && meta.size() == held.witness.size
            && meta.mtime() == held.witness.mtime
            && meta.mtime_nsec() == held.witness.mtime_ns
            && meta.nlink() == 1
            && AnchoredPath::open(Path::new(original).parent().unwrap()).is_ok_and(|parent| {
                parent.witness.nodes == held.witness.nodes[..held.witness.nodes.len() - 1]
            })
            && std::fs::symlink_metadata(original)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
        Ok(if matches {
            (
                journal::Outcome::Moved(receipt),
                "fixture moved and metadata verified".into(),
            )
        } else {
            (
                journal::Outcome::Uncertain(Some(receipt)),
                "fixture result changed; stop for manual inspection".into(),
            )
        })
    }

    fn detail(&self, run: &str) -> journal::Detail {
        journal::detail(&self.fixture.db, run).unwrap()
    }

    fn reopen(&mut self) {
        self.fixture.db = db::open_fixture(&self.fixture.root.join("app.sqlite")).unwrap();
    }

    fn only_run(&self) -> String {
        let runs = journal::list(&self.fixture.db).unwrap();
        assert_eq!(runs.len(), 1);
        runs[0].id.clone()
    }
}

fn no_fault(_: Phase, _: usize, _: &mut Harness) -> Result<(), String> {
    Ok(())
}

fn states(detail: &journal::Detail) -> Vec<&str> {
    detail
        .items
        .iter()
        .map(|item| item.state.as_str())
        .collect()
}

#[test]
fn selected_plan_drives_real_moves_and_sqlite_results_without_touching_unselected_files() {
    let mut h = Harness::new();
    h.permit_fixture_rules();
    let confirmation = h.confirm();
    let run = h.run(&confirmation, no_fault).unwrap();
    let detail = h.detail(&run);
    assert_eq!(detail.summary.state, "completed");
    assert_eq!(states(&detail), ["moved", "moved"]);
    assert_eq!(detail.events.len(), 4);
    assert_eq!(detail.snapshot_sha256, confirmation.digest);
    for (index, item) in detail.items.iter().enumerate() {
        assert!(!Path::new(&item.path).exists());
        let receipt = item.receipt.as_ref().unwrap();
        assert_eq!(
            std::fs::read_to_string(&receipt.destination).unwrap(),
            if index == 0 {
                "中文\ncache *"
            } else {
                "second"
            }
        );
    }
    assert!(h.fixture.root.join("source/unselected").is_file());
    assert!(h.run(&confirmation, no_fault).is_err());
    h.reopen();
    assert_eq!(states(&h.detail(&run)), ["moved", "moved"]);
}

#[test]
fn stale_changed_unknown_and_foreign_confirmations_never_create_an_execution() {
    let mut h = Harness::new();
    h.permit_fixture_rules();
    let mut confirmation = h.confirm();
    confirmation.digest.push('0');
    assert!(h.run(&confirmation, no_fault).is_err());
    confirmation = h.confirm();
    h.fixture.at = confirmation.expires_at;
    assert!(h.run(&confirmation, no_fault).is_err());
    h.fixture.at = confirmation.confirmed_at - 1;
    assert!(h.run(&confirmation, no_fault).is_err());
    h.fixture.at = confirmation.confirmed_at;
    confirmation.plan_id = uuid::Uuid::new_v4().to_string();
    assert!(h.run(&confirmation, no_fault).is_err());
    assert!(journal::list(&h.fixture.db).unwrap().is_empty());
    assert!(h.fixture.root.join("source/second").is_file());
}

#[test]
fn changed_persisted_selection_and_foreign_roots_are_rejected_before_begin() {
    let mut h = Harness::new();
    h.permit_fixture_rules();
    let confirmation = h.confirm();
    let mut changed = h.fixture.plan.clone();
    changed.items.reverse();
    h.fixture
        .db
        .execute(
            "UPDATE review_plans SET body=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&changed).unwrap(), changed.id],
        )
        .unwrap();
    assert!(h
        .run(&confirmation, no_fault)
        .unwrap_err()
        .contains("confirmation"));
    // Even a newly confirmed snapshot cannot grant access outside this harness.
    changed.items[0].candidate.path = h
        .fixture
        .root
        .join("outside-source")
        .to_str()
        .unwrap()
        .into();
    std::fs::write(
        &changed.items[0].candidate.path,
        "retained outside fixture scope",
    )
    .unwrap();
    h.fixture
        .db
        .execute(
            "UPDATE review_plans SET body=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&changed).unwrap(), changed.id],
        )
        .unwrap();
    assert!(h
        .run(&h.confirm(), no_fault)
        .unwrap_err()
        .contains("outside"));
    assert!(journal::list(&h.fixture.db).unwrap().is_empty());
    assert!(h.fixture.root.join("source/second").is_file());
    assert!(Path::new(&changed.items[0].candidate.path).is_file());
}

#[test]
fn unreadable_protection_aborts_before_intent_instead_of_treating_it_as_empty() {
    let mut h = Harness::new();
    h.permit_fixture_rules();
    assert!(h
        .run(&h.confirm(), |phase, index, h| {
            if (phase, index) == (Phase::BeforeIntent, 0) {
                h.fixture
                    .db
                    .execute("INSERT INTO settings VALUES (1, 'broken-json')", [])
                    .unwrap();
            }
            Ok(())
        })
        .is_err());
    let run = h.only_run();
    assert!(h.detail(&run).events.is_empty());
    // Startup also refuses corrupt settings. Its committed reconciliation still
    // preserves the no-intent evidence; it never substitutes empty protection.
    assert!(db::open_fixture(&h.fixture.root.join("app.sqlite")).is_err());
    assert_eq!(states(&h.detail(&run)), ["notStarted", "notStarted"]);
    assert!(Path::new(&h.fixture.plan.items[0].candidate.path).is_file());
    assert!(h.fixture.root.join("source/second").is_file());
}

#[test]
fn current_protection_and_unknown_rules_skip_without_intent_then_allow_other_items() {
    for rule in [
        None,
        Some(FixtureRule::UnknownFixture),
        Some(FixtureRule::RetainFixture),
    ] {
        let mut h = Harness::new();
        h.permit_fixture_rules();
        let id = h.fixture.item(0);
        h.rules.remove(&id);
        if let Some(rule) = rule {
            h.rules.insert(id, rule);
        }
        let run = h.run(&h.confirm(), no_fault).unwrap();
        let detail = h.detail(&run);
        assert_eq!(detail.summary.state, "partial");
        assert_eq!(states(&detail), ["skipped", "moved"]);
        assert_eq!(detail.events.len(), 3);
    }
    let mut h = Harness::new();
    h.permit_fixture_rules();
    let run = h
        .run(&h.confirm(), |phase, index, h| {
            if phase == Phase::BeforeCheck && index == 1 {
                let mut settings = db::settings(&h.fixture.db)?;
                settings
                    .protected_paths
                    .push(h.fixture.plan.items[1].candidate.path.clone());
                db::save_settings(&h.fixture.db, &settings)?;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(states(&h.detail(&run)), ["moved", "skipped"]);
    assert!(h.fixture.root.join("source/second").is_file());
}

#[test]
fn cancellation_before_and_after_intent_preserves_actual_results() {
    for (phase, index, expected) in [
        (Phase::BeforeIntent, 0, vec!["cancelled", "cancelled"]),
        (Phase::AfterIntent, 0, vec!["moved", "cancelled"]),
        (Phase::AfterResult, 1, vec!["moved", "moved"]),
    ] {
        let mut h = Harness::new();
        h.permit_fixture_rules();
        let run = h
            .run(&h.confirm(), |current, i, h| {
                if (current, i) == (phase, index) {
                    h.cancelled = true;
                }
                Ok(())
            })
            .unwrap();
        let detail = h.detail(&run);
        assert_eq!(states(&detail), expected);
        assert_eq!(
            detail.summary.state,
            if phase == Phase::AfterResult {
                "completed"
            } else {
                "cancelled"
            }
        );
    }
}

#[test]
fn conflict_is_a_partial_failure_and_does_not_overwrite_recovery_files() {
    let mut h = Harness::new();
    h.permit_fixture_rules();
    std::fs::write(h.fixture.root.join("recovery/second"), "existing").unwrap();
    let run = h.run(&h.confirm(), no_fault).unwrap();
    let detail = h.detail(&run);
    assert_eq!(detail.summary.state, "partial");
    assert_eq!(states(&detail), ["moved", "failed"]);
    assert_eq!(
        std::fs::read_to_string(h.fixture.root.join("recovery/second")).unwrap(),
        "existing"
    );
    assert_eq!(
        std::fs::read_to_string(h.fixture.root.join("source/second")).unwrap(),
        "second"
    );
}

#[test]
fn validation_after_checks_rejects_late_protection_rule_and_time_changes() {
    for change in 0..3 {
        let mut h = Harness::new();
        h.permit_fixture_rules();
        let run = h
            .run(&h.confirm(), |phase, index, h| {
                if phase == Phase::BeforeIntent && index == 0 {
                    match change {
                        0 => {
                            let mut settings = db::settings(&h.fixture.db)?;
                            settings
                                .protected_paths
                                .push(h.fixture.plan.items[0].candidate.path.clone());
                            db::save_settings(&h.fixture.db, &settings)?;
                        }
                        1 => {
                            h.rules
                                .insert(h.fixture.item(0), FixtureRule::UnknownFixture);
                        }
                        _ => {
                            h.fixture.at += CONFIRMATION_LIFETIME;
                        }
                    }
                }
                Ok(())
            })
            .unwrap();
        let detail = h.detail(&run);
        assert_eq!(detail.items[0].state, "skipped");
        assert!(detail
            .events
            .iter()
            .all(|event| event.item_id != h.fixture.item(0) || event.state == "skipped"));
        assert!(Path::new(&h.fixture.plan.items[0].candidate.path).is_file());
        assert_eq!(
            detail.items[1].state,
            if change == 2 { "skipped" } else { "moved" }
        );
    }
}

#[test]
fn missing_linked_and_changed_sources_are_retained_before_any_intent() {
    for change in 0..4 {
        let mut h = Harness::new();
        h.permit_fixture_rules();
        let run = h
            .run(&h.confirm(), |phase, index, h| {
                if phase == Phase::BeforeCheck && index == 0 {
                    let source = Path::new(&h.fixture.plan.items[0].candidate.path);
                    match change {
                        0 => std::fs::remove_file(source).unwrap(),
                        1 => {
                            std::fs::remove_file(source).unwrap();
                            std::os::unix::fs::symlink(
                                h.fixture.root.join("source/unselected"),
                                source,
                            )
                            .unwrap();
                        }
                        2 => std::fs::hard_link(source, h.fixture.root.join("extra-link")).unwrap(),
                        _ => std::fs::write(source, "changed").unwrap(),
                    }
                }
                Ok(())
            })
            .unwrap();
        let detail = h.detail(&run);
        assert_eq!(states(&detail), ["skipped", "moved"]);
        assert!(h.fixture.root.join("source/unselected").is_file());
    }
}

#[test]
fn journal_failure_before_intent_prevents_move_and_after_move_remains_uncertain() {
    for phase in [Phase::BeforeIntent, Phase::AfterMove] {
        let mut h = Harness::new();
        h.permit_fixture_rules();
        assert!(h
            .run(&h.confirm(), |current, index, h| {
                if current == phase && index == 0 {
                    h.fixture
                        .db
                        .pragma_update(None, "query_only", true)
                        .unwrap();
                }
                Ok(())
            })
            .is_err());
        let run = h.only_run();
        assert_eq!(
            h.detail(&run).items[0].state,
            if phase == Phase::BeforeIntent {
                "pending"
            } else {
                "intent"
            }
        );
        h.reopen();
        let detail = h.detail(&run);
        assert_eq!(detail.summary.state, "interrupted");
        assert_eq!(
            states(&detail),
            if phase == Phase::BeforeIntent {
                vec!["notStarted", "notStarted"]
            } else {
                vec!["uncertain", "notStarted"]
            }
        );
        assert_eq!(
            Path::new(&h.fixture.plan.items[0].candidate.path).exists(),
            phase == Phase::BeforeIntent
        );
        assert!(h.fixture.root.join("source/second").is_file());
    }
}

#[test]
fn interruption_at_operation_boundaries_is_not_replayed_on_restart() {
    for phase in [Phase::AfterIntent, Phase::AfterMove, Phase::AfterResult] {
        let mut h = Harness::new();
        h.permit_fixture_rules();
        let confirmation = h.confirm();
        assert!(h
            .run(&confirmation, |current, index, _| {
                if (current, index) == (phase, 0) {
                    Err("injected driver interruption".into())
                } else {
                    Ok(())
                }
            })
            .is_err());
        let run = h.only_run();
        h.reopen();
        let detail = h.detail(&run);
        assert_eq!(detail.summary.state, "interrupted");
        assert_eq!(
            states(&detail),
            if phase == Phase::AfterResult {
                vec!["moved", "notStarted"]
            } else {
                vec!["uncertain", "notStarted"]
            }
        );
        assert!(h.run(&confirmation, no_fault).is_err());
        assert!(h.fixture.root.join("source/second").is_file());
    }
}

#[test]
fn final_source_name_race_is_detected_and_stops_batch_but_is_not_prevented() {
    let mut h = Harness::new();
    h.permit_fixture_rules();
    let run = h
        .run(&h.confirm(), |phase, index, h| {
            if (phase, index) == (Phase::AfterIntent, 0) {
                let original = &h.fixture.plan.items[0].candidate.path;
                std::fs::rename(original, h.fixture.root.join("original-retained")).unwrap();
                std::fs::write(original, "replacement").unwrap();
            }
            Ok(())
        })
        .unwrap();
    let detail = h.detail(&run);
    assert_eq!(detail.summary.state, "interrupted");
    assert_eq!(states(&detail), ["uncertain", "notStarted"]);
    let receipt = detail.items[0].receipt.as_ref().unwrap();
    assert_eq!(
        std::fs::read_to_string(&receipt.destination).unwrap(),
        "replacement"
    );
    assert_eq!(
        std::fs::read_to_string(h.fixture.root.join("original-retained")).unwrap(),
        "中文\ncache *"
    );
    assert!(h.fixture.root.join("source/second").is_file());
    // This proves why the driver MUST remain test-only. Recording uncertainty
    // does not undo moving an object that was never selected.
}

#[test]
fn parent_replacement_after_intent_does_not_publish_success_or_start_next_item() {
    let mut h = Harness::new();
    h.permit_fixture_rules();
    let run = h
        .run(&h.confirm(), |phase, index, h| {
            if (phase, index) == (Phase::AfterIntent, 0) {
                std::fs::rename(
                    h.fixture.root.join("source"),
                    h.fixture.root.join("parent-renamed"),
                )
                .unwrap();
                std::fs::create_dir(h.fixture.root.join("source")).unwrap();
            }
            Ok(())
        })
        .unwrap();
    let detail = h.detail(&run);
    assert_eq!(detail.summary.state, "interrupted");
    assert_eq!(states(&detail), ["uncertain", "notStarted"]);
    assert!(detail.items[0].receipt.is_some());
    assert!(h.fixture.root.join("parent-renamed/second").is_file());
    assert!(h.fixture.root.join("parent-renamed/unselected").is_file());
}

#[test]
#[ignore = "Explicit native Trash + SQLite workflow: only fresh private fixtures; exact receipts restored without overwriting"]
fn native_trash_sqlite_workflow_restores_exact_receipts_and_preserves_collisions() {
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    autoreleasepool(|_| {
        let mut h = Harness::new();
        h.permit_fixture_rules();
        h.backend = Backend::NativeTrash;
        // Keep recovery evidence even if the API check panics/fails.
        h.fixture._root.disable_cleanup(true);
        let rescue = h.fixture.root.join("native-workflow-rescue.json");
        let root = h.fixture.root.clone();
        eprintln!(
            "Native workflow fixture root (preserved on failure): {}",
            root.display()
        );
        let confirmation = h.confirm();
        let result = h.run(&confirmation, no_fault);
        let report = journal::list(&h.fixture.db).unwrap();
        std::fs::write(&rescue, serde_json::to_vec_pretty(&serde_json::json!({
            "fixtureRoot": root, "runs": report,
            "details": report.iter().map(|run| serde_json::to_value(h.detail(&run.id)).unwrap()).collect::<Vec<_>>()
        })).unwrap()).unwrap();
        // Disable TempDir removal until all native fixtures are restored. A
        // failed assertion leaves the SQLite journal and exact return paths.
        eprintln!("Native workflow recovery evidence: {}", rescue.display());
        let run = result.unwrap();
        h.reopen();
        let detail = h.detail(&run);
        assert_eq!(detail.summary.state, "completed");
        assert_eq!(states(&detail), ["moved", "moved"]);
        let manager = NSFileManager::defaultManager();
        for (index, item) in detail.items.iter().enumerate() {
            let receipt = item.receipt.as_ref().unwrap();
            let meta = std::fs::symlink_metadata(&receipt.destination).unwrap();
            assert_eq!((meta.dev(), meta.ino()), (receipt.device, receipt.inode));
            let from = NSURL::fileURLWithPath(&NSString::from_str(&receipt.destination));
            let to = NSURL::fileURLWithPath(&NSString::from_str(&item.path));
            if index == 0 {
                std::fs::write(&item.path, "new file retained").unwrap();
                assert!(manager.moveItemAtURL_toURL_error(&from, &to).is_err());
                assert_eq!(
                    std::fs::read_to_string(&item.path).unwrap(),
                    "new file retained"
                );
                assert!(Path::new(&receipt.destination).is_file());
                std::fs::rename(&item.path, root.join("collision-retained")).unwrap();
            }
            manager.moveItemAtURL_toURL_error(&from, &to).unwrap();
            let meta = std::fs::symlink_metadata(&item.path).unwrap();
            assert_eq!((meta.dev(), meta.ino()), (receipt.device, receipt.inode));
            assert!(!Path::new(&receipt.destination).exists());
        }
        assert!(root.join("source/unselected").is_file());
        assert_eq!(
            std::fs::read_to_string(&detail.items[0].path).unwrap(),
            "中文\ncache *"
        );
        // Manual fixture recovery is not recorded as a product restore action.
        // No protocol for user restoration has been opened by this test.
        h.fixture._root.disable_cleanup(false);
        eprintln!("Native selected-file workflow: SQLite receipts persisted; 2 fixtures restored; collision and unselected file retained.");
    });
}

#[test]
#[ignore = "Explicit prepared native gate + SQLite check: private fixture changes after intent must not invoke Trash"]
fn native_prepared_gate_rejects_after_intent_changes_without_moving_files() {
    objc2::rc::autoreleasepool(|_| {
        for kind in ["name", "parent", "content"] {
            let mut h = Harness::new();
            h.permit_fixture_rules();
            h.backend = Backend::NativeTrash;
            h.fixture._root.disable_cleanup(true);
            eprintln!(
                "Native journal gate fixture {kind}: {}",
                h.fixture.root.display()
            );
            let mut retained_original = h.fixture.plan.items[0].candidate.path.clone();
            let run = h
                .run(&h.confirm(), |phase, index, h| {
                    if (phase, index) == (Phase::AfterIntent, 0) {
                        let original = &h.fixture.plan.items[0].candidate.path;
                        match kind {
                            "name" => {
                                let relocated = h.fixture.root.join("relocated-selected");
                                std::fs::rename(original, &relocated).unwrap();
                                std::fs::write(original, "replacement retained").unwrap();
                                retained_original = relocated.to_str().unwrap().into();
                            }
                            "parent" => {
                                let relocated = h.fixture.root.join("relocated-parent");
                                std::fs::rename(h.fixture.root.join("source"), &relocated).unwrap();
                                std::fs::create_dir(h.fixture.root.join("source")).unwrap();
                                std::fs::write(original, "replacement retained").unwrap();
                                retained_original = relocated
                                    .join(Path::new(original).file_name().unwrap())
                                    .to_str()
                                    .unwrap()
                                    .into();
                            }
                            _ => std::fs::write(original, "changed selected retained").unwrap(),
                        }
                        // Stop later items so this check must not move ANY file.
                        h.cancelled = true;
                    }
                    Ok(())
                })
                .unwrap();
            let detail = h.detail(&run);
            std::fs::write(h.fixture.root.join("native-gate-rescue.json"), serde_json::to_vec_pretty(&serde_json::json!({
                "fixtureRoot": h.fixture.root, "change": kind, "retainedOriginal": retained_original,
                "detail": detail,
            })).unwrap()).unwrap();
            assert_eq!(states(&detail), ["failed", "cancelled"]);
            assert_eq!(detail.summary.state, "cancelled");
            assert!(detail.items[0].reason.starts_with("native API not called:"));
            assert!(detail.items.iter().all(|item| item.receipt.is_none()));
            assert_eq!(detail.events.len(), 3);
            assert_eq!(
                std::fs::read_to_string(&retained_original).unwrap(),
                if kind == "content" {
                    "changed selected retained"
                } else {
                    "中文\ncache *"
                }
            );
            if kind != "content" {
                assert_eq!(
                    std::fs::read_to_string(&detail.items[0].path).unwrap(),
                    "replacement retained"
                );
            }
            h.reopen();
            assert_eq!(states(&h.detail(&run)), ["failed", "cancelled"]);
            let source = h.fixture.root.join(if kind == "parent" {
                "relocated-parent"
            } else {
                "source"
            });
            assert_eq!(
                std::fs::read_to_string(source.join("second")).unwrap(),
                "second"
            );
            assert_eq!(
                std::fs::read_to_string(source.join("unselected")).unwrap(),
                "unselected"
            );
            h.fixture._root.disable_cleanup(false);
            eprintln!("Native journal gate {kind}: no Trash call, no receipt, cancellation preserved after restart.");
        }
    });
}
