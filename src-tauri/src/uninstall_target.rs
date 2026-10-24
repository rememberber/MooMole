//! Whole-App native building block, called by the sealed uninstall executor.
//! Identity and filesystem access are NOT uninstall authority. A sealed caller
//! must first commit intent and supply fresh confirmation/runtime/policy gates.
#![allow(dead_code)]
use crate::{
    app_inspection::identity,
    observation::{AnchoredPath, Witness},
    permissions,
    uninstall_plan::{Context, Plan},
};
use objc2::rc::{autoreleasepool, Retained};
use objc2_foundation::{NSFileManager, NSString, NSURL};
use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(crate) struct PreparedBundle {
    path: PathBuf,
    snapshot: identity::Snapshot,
    created_at: i64,
    expires_at: i64,
    uid: u32,
    held: AnchoredPath,
    reference: Retained<NSURL>,
}

// Distinct from an ordinary-file outcome/receipt. The minimal native location
// shares only the lossless decimal device/inode encoding, never its authority.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    pub version: u32,
    pub actual: crate::execution::Receipt,
    pub observed_inventory: Option<crate::tree::BundleWitness>,
}

#[derive(Debug)]
pub(crate) enum Outcome {
    Moved(Receipt),
    Uncertain(Option<Receipt>),
}

#[derive(Debug)]
pub(crate) struct Attempt {
    pub outcome: Outcome,
    pub reason: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
enum Preparation {
    Anchored,
    Referenced,
}

struct NativeResult {
    succeeded: bool,
    destination: Option<PathBuf>,
}

impl PreparedBundle {
    #[cfg(test)]
    pub(crate) fn move_to_private_checked(
        &self,
        destination: &Path,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Result<Attempt, String> {
        self.trash_with(gate, |_| {
            let result = std::fs::rename(&self.path, destination);
            NativeResult {
                succeeded: result.is_ok(),
                destination: result.ok().map(|_| destination.to_owned()),
            }
        })
    }
    pub(crate) fn prepare(plan: &Plan, ctx: &Context) -> Result<Self, String> {
        plan.recheck(ctx, crate::db::now())?;
        let (path, snapshot, created_at, expires_at) = plan.bundle_binding();
        autoreleasepool(|_| {
            Self::prepare_with(Path::new(path), snapshot, created_at, expires_at, |_| {})
        })
    }

    fn prepare_with(
        path: &Path,
        snapshot: &identity::Snapshot,
        created_at: i64,
        expires_at: i64,
        mut checkpoint: impl FnMut(Preparation),
    ) -> Result<Self, String> {
        if snapshot.version != 1 || snapshot.inventory.version != 1 {
            return Err("App 包身份版本不支持，请重新生成计划。".into());
        }
        let held = AnchoredPath::open(path)?;
        let meta = held
            .target()
            .metadata()
            .map_err(|_| "无法读取 App 包身份。")?;
        let uid = unsafe { libc::geteuid() };
        if uid == 0
            || !meta.is_dir()
            || meta.uid() != uid
            || held.witness != snapshot.root
            || !held.local_materialized()
        {
            return Err("App 包已变化或不属于当前普通账户的本地目录范围。".into());
        }
        checkpoint(Preparation::Anchored);
        let url = NSURL::fileURLWithPath(&NSString::from_str(
            path.to_str().ok_or("App 路径无法编码。")?,
        ));
        let reference = url
            .fileReferenceURL()
            .filter(|url| url.isFileReferenceURL())
            .ok_or("系统无法为 App 包建立对象引用。")?;
        checkpoint(Preparation::Referenced);
        let prepared = Self {
            path: path.to_owned(),
            snapshot: snapshot.clone(),
            created_at,
            expires_at,
            uid,
            held,
            reference,
        };
        prepared.validate()?;
        Ok(prepared)
    }

    fn check_time(&self) -> Result<(), String> {
        let now = crate::db::now();
        if now < self.created_at || now >= self.expires_at || self.uid != unsafe { libc::geteuid() }
        {
            return Err("卸载计划已过期或账户变化，请重新准备。".into());
        }
        Ok(())
    }

    fn validate_reference(&self) -> Result<(), String> {
        self.check_time()?;
        self.held.validate()?;
        let resolved = self
            .reference
            .filePathURL()
            .filter(|url| url.isFileURL())
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()));
        if resolved.as_deref() != Some(self.path.as_path()) {
            return Err("App 包已移位，不能跟随对象引用到其他位置处理。".into());
        }
        let current = AnchoredPath::open(&self.path)?;
        if current.witness != self.snapshot.root || !current.local_materialized() {
            return Err("App 包或祖先身份已变化，请重新生成计划。".into());
        }
        self.held.validate()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        autoreleasepool(|_| {
            self.validate_reference()?;
            if identity::capture(&self.path, &self.snapshot.bundle_id)? != self.snapshot {
                return Err("App 包内容、身份或主程序已变化，请重新生成计划。".into());
            }
            self.validate_reference()
        })
    }

    /// Any Err is before the native API. A caller must persist an intent first,
    /// then record a pre-call refusal; after any API call only evidence-based
    /// Moved/Uncertain is allowed, even when Foundation returns an error.
    pub(crate) fn move_to_trash_checked(
        &self,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Result<Attempt, String> {
        autoreleasepool(|_| {
            self.trash_with(gate, |reference| {
                let mut actual = None;
                let result = NSFileManager::defaultManager()
                    .trashItemAtURL_resultingItemURL_error(reference, Some(&mut actual));
                NativeResult {
                    succeeded: result.is_ok(),
                    destination: actual
                        .filter(|url| url.isFileURL())
                        .and_then(|url| url.path())
                        .map(|path| PathBuf::from(path.to_string())),
                }
            })
        })
    }

    fn trash_with(
        &self,
        gate: impl FnOnce() -> Result<(), String>,
        native: impl FnOnce(&NSURL) -> NativeResult,
    ) -> Result<Attempt, String> {
        self.validate()?;
        let access = permissions::observe(&self.held, &permissions::Budget::new())?;
        if access.code != permissions::Code::NoRestrictionObserved {
            return Err(access.refusal_reason());
        }
        self.validate()?;
        // The owner supplies latest settings/runtime/confirmation/cancellation
        // and durable-intent validation here. It must not renew a stale plan.
        gate()?;
        self.check_time()?;
        let result = native(&self.reference);
        // No fallible propagation beyond this point: the OS may have changed
        // something even on error or when the returned evidence is missing.
        Ok(self.observe_result(result))
    }

    fn observe_result(&self, result: NativeResult) -> Attempt {
        let destination = result.destination.filter(|path| {
            path.is_absolute()
                && path != &self.path
                && path.to_str().is_some_and(|s| s.len() <= 4096)
        });
        let after = destination
            .as_ref()
            .and_then(|path| identity::capture_returned(path, &self.snapshot.bundle_id).ok());
        let receipt = destination.as_ref().and_then(|path| {
            // lstat records the returned object itself, never a linked target.
            std::fs::symlink_metadata(path).ok().map(|m| Receipt {
                version: 1,
                actual: crate::execution::Receipt {
                    destination: path.to_string_lossy().into_owned(),
                    device: m.dev(),
                    inode: m.ino(),
                },
                observed_inventory: after.as_ref().map(|snapshot| snapshot.inventory.clone()),
            })
        });
        let parent_unchanged = self.path.parent().is_some_and(|parent| {
            AnchoredPath::open(parent).is_ok_and(|parent| {
                parent.witness.nodes
                    == self.snapshot.root.nodes[..self.snapshot.root.nodes.len() - 1]
                    && parent.local_materialized()
            })
        });
        let source_absent = std::fs::symlink_metadata(&self.path)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
        let verified = destination
            .as_ref()
            .zip(after.as_ref())
            .is_some_and(|(path, after)| {
                relocated_matches(&self.snapshot, after, &self.path, path)
                    && receipt.as_ref().is_some_and(|receipt| {
                        after.root.nodes.last().is_some_and(|node| {
                            receipt.actual.device == node.dev && receipt.actual.inode == node.ino
                        })
                    })
            });
        if result.succeeded && parent_unchanged && source_absent && verified {
            if let Some(receipt) = receipt {
                return Attempt {
                    outcome: Outcome::Moved(receipt),
                    reason:
                        "App 包已移入废纸篓，实际目标身份、包清单及原位置已核对；包外关联数据保留。",
                };
            }
        }
        Attempt {
            outcome: Outcome::Uncertain(receipt),
            reason: "系统处理已尝试，但结果或包内容/原位置无法完整核对；结果待核实，不得自动重试。",
        }
    }
}

// After a same-device relocation the root ctime/path changes. All bound
// descendants, their metadata, Info.plist bytes and the inventory must still
// match. Destination ancestors are new and checked by capture, not compared to
// source ancestors; source parent identity is checked separately above.
fn relocated_witness(
    before: &Witness,
    after: &Witness,
    source: &Path,
    destination: &Path,
    root: bool,
) -> bool {
    let before_nodes: Vec<_> = before
        .nodes
        .iter()
        .filter_map(|node| {
            node.path
                .strip_prefix(source)
                .ok()
                .map(|relative| (relative, node))
        })
        .collect();
    let after_nodes: Vec<_> = after
        .nodes
        .iter()
        .filter_map(|node| {
            node.path
                .strip_prefix(destination)
                .ok()
                .map(|relative| (relative, node))
        })
        .collect();
    !before_nodes.is_empty()
        && before_nodes.len() == after_nodes.len()
        && before_nodes
            .iter()
            .zip(&after_nodes)
            .all(|((a, n), (b, m))| {
                a == b
                    && n.dev == m.dev
                    && n.ino == m.ino
                    && n.mode == m.mode
                    && n.uid == m.uid
                    && n.gid == m.gid
            })
        && before.version == after.version
        && before.size == after.size
        && before.links == after.links
        && before.mtime == after.mtime
        && before.mtime_ns == after.mtime_ns
        && (root || (before.ctime == after.ctime && before.ctime_ns == after.ctime_ns))
}

fn relocated_matches(
    before: &identity::Snapshot,
    after: &identity::Snapshot,
    source: &Path,
    destination: &Path,
) -> bool {
    before.version == after.version
        && before.bundle_id == after.bundle_id
        && before.app_version == after.app_version
        && before.build == after.build
        && before.background == after.background
        && before.executable_name == after.executable_name
        && before.info_sha256 == after.info_sha256
        && before.inventory == after.inventory
        && relocated_witness(&before.root, &after.root, source, destination, true)
        && relocated_witness(
            &before.contents,
            &after.contents,
            source,
            destination,
            false,
        )
        && relocated_witness(&before.info, &after.info, source, destination, false)
        && relocated_witness(
            &before.executable,
            &after.executable,
            source,
            destination,
            false,
        )
}

#[cfg(test)]
mod tests;
