//! Bind a short-lived native reference to an observed ordinary file or strict cache tree.
//! This building block grants no cleanup authority: it checks neither Mole
//! module rules nor user confirmation. Only the sealed cleanup executor may use it.
//! Reference identity and path scope are separate, and validation is not a lock.
use crate::{observation::AnchoredPath, review::ScanItem};
use objc2::rc::{autoreleasepool, Retained};
use objc2_foundation::{NSString, NSURL};
use std::{os::unix::fs::MetadataExt, path::PathBuf};

pub(crate) struct PreparedFile {
    held: AnchoredPath,
    path: PathBuf,
    reference: Retained<NSURL>,
    tree: Option<crate::tree::TreeWitness>,
    contents: Option<crate::tree::TreeWitness>,
}

#[derive(Clone, Copy, PartialEq)]
enum Preparation {
    Anchored,
    Referenced,
}

impl PreparedFile {
    pub(crate) fn prepare(item: &ScanItem) -> Result<Self, String> {
        autoreleasepool(|_| Self::prepare_with(item, |_| {}))
    }

    fn prepare_with(
        item: &ScanItem,
        mut checkpoint: impl FnMut(Preparation),
    ) -> Result<Self, String> {
        let expected = item
            .witness
            .as_ref()
            .ok_or("缺少扫描时的文件身份，请重新扫描。")?;
        if expected.version != crate::observation::VERSION {
            return Err("需要当前版本的文件或目录身份，请重新扫描。".into());
        }
        let path = PathBuf::from(&item.candidate.path);
        let held = AnchoredPath::open(&path)?;
        let meta = held.target().metadata().map_err(|e| e.to_string())?;
        if held.witness != *expected
            || !(meta.is_file() || meta.is_dir())
            || meta.is_dir() != item.candidate.is_directory
            || (meta.is_file() && meta.nlink() != 1)
            || meta.uid() != unsafe { libc::geteuid() }
            || !held.local_materialized()
        {
            return Err("文件已变化、不是本账户的单链接普通文件，或不在已落地的本地范围。".into());
        }
        checkpoint(Preparation::Anchored);
        let url = NSURL::fileURLWithPath(&NSString::from_str(&item.candidate.path));
        let reference = url
            .fileReferenceURL()
            .ok_or("系统无法为此文件建立对象引用。")?;
        if !reference.isFileReferenceURL() {
            return Err("系统未返回文件对象引用。".into());
        }
        checkpoint(Preparation::Referenced);
        let tree = if meta.is_dir() {
            let expected_tree = item
                .tree
                .as_ref()
                .filter(|t| t.version == crate::tree::VERSION)
                .ok_or("缓存目录缺少完整内容快照。")?;
            let actual = crate::tree::inspect(
                &path,
                meta.dev(),
                meta.ino(),
                &mut crate::tree::Budget::new(),
            )?;
            if &actual != expected_tree {
                return Err("缓存目录内容已变化。".into());
            }
            Some(actual)
        } else {
            None
        };
        let contents = if meta.is_dir() {
            Some(crate::tree::inspect_contents(
                &path,
                meta.dev(),
                meta.ino(),
            )?)
        } else {
            None
        };
        let prepared = Self {
            held,
            path,
            reference,
            tree,
            contents,
        };
        prepared.validate()?;
        Ok(prepared)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        autoreleasepool(|_| self.validate_inner())
    }

    fn validate_inner(&self) -> Result<(), String> {
        self.held.validate()?;
        let resolved = self
            .reference
            .filePathURL()
            .ok_or("文件对象引用已无法定位。")?;
        let path = resolved.path().ok_or("文件对象引用没有本地路径。")?;
        if !resolved.isFileURL() || PathBuf::from(path.to_string()) != self.path {
            return Err("选中文件的位置已变化，请重新扫描；不会跟随到新位置处理。".into());
        }
        // A reference made after a path replacement could point at the new file.
        // Compare the complete planned observation, not just the resolved name.
        let current = AnchoredPath::open(&self.path)?;
        if current.witness != self.held.witness || !current.local_materialized() {
            return Err("文件引用与原计划身份或本地范围不一致，请重新扫描。".into());
        }
        if let Some(expected) = &self.tree {
            let node = self.held.witness.nodes.last().unwrap();
            let actual = crate::tree::inspect(
                &self.path,
                node.dev,
                node.ino,
                &mut crate::tree::Budget::new(),
            )?;
            if &actual != expected {
                return Err("缓存目录深层内容已变化，保留。".into());
            }
        }
        self.held.validate()
    }
}

pub(crate) struct Attempt {
    pub outcome: crate::execution::Outcome,
    pub reason: String,
}

impl PreparedFile {
    pub(crate) fn move_to_trash(&self) -> Result<Attempt, String> {
        self.move_to_trash_checked(|| Ok(()))
    }

    pub(crate) fn move_to_trash_checked(
        &self,
        gate: impl FnOnce() -> Result<(), String>,
    ) -> Result<Attempt, String> {
        autoreleasepool(|_| self.trash_with(gate))
    }

    /// Test-only operation, restricted to the current fixture's direct children.
    /// `after_gate` deliberately exposes the remaining non-atomic window. Never
    /// promote this method merely because the ordinary case passes.
    #[cfg(test)]
    pub(crate) fn trash_fixture(
        &self,
        fixture_root: &std::path::Path,
        after_gate: impl FnOnce(),
    ) -> Result<Attempt, String> {
        if self.path.parent() != Some(fixture_root.join("source").as_path()) {
            return Err("native experiment is outside this fixture source".into());
        }
        self.trash_with(|| {
            after_gate();
            Ok(())
        })
    }

    // Any Err is strictly BEFORE the native call. Once called, missing/error
    // evidence is an Uncertain result, never a known-no-change failure.
    fn trash_with(
        &self,
        after_gate: impl FnOnce() -> Result<(), String>,
    ) -> Result<Attempt, String> {
        use crate::execution::{Outcome, Receipt};
        use objc2_foundation::NSFileManager;
        self.validate()?;
        // Parent ACLs/flags may change without changing the path witness's
        // identity/mode. Re-observe both held objects at the final gate; unknown
        // is not authority. A later OS race still remains non-atomic.
        let access = crate::permissions::observe(&self.held, &crate::permissions::Budget::new())?;
        if access.code != crate::permissions::Code::NoRestrictionObserved {
            return Err(access.refusal_reason());
        }
        self.validate()?;
        after_gate()?;
        let mut actual = None;
        let result = NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&self.reference, Some(&mut actual));
        let destination = actual
            .and_then(|url| url.path())
            .map(|p| PathBuf::from(p.to_string()));
        let receipt = destination.as_ref().and_then(|path| {
            std::fs::symlink_metadata(path).ok().map(|m| Receipt {
                destination: path.to_string_lossy().into_owned(),
                device: m.dev(),
                inode: m.ino(),
            })
        });
        if let Err(error) = result {
            return Ok(Attempt {
                outcome: Outcome::Uncertain(receipt),
                reason: format!(
                    "系统移入废纸篓返回错误，结果待核实：{} ({}, {})",
                    error.localizedDescription(),
                    error.domain(),
                    error.code()
                ),
            });
        }
        let before = &self.held.witness;
        let node = before.nodes.last().unwrap();
        let verified = destination.as_ref().is_some_and(|path| {
            std::fs::symlink_metadata(path).is_ok_and(|m| {
                (if self.tree.is_some() {
                    m.is_dir()
                } else {
                    m.is_file()
                }) && m.dev() == node.dev
                    && m.ino() == node.ino
                    && m.mode() == node.mode
                    && m.uid() == node.uid
                    && m.gid() == node.gid
                    && m.nlink() == before.links
                    && m.size() == before.size
                    && m.mtime() == before.mtime
                    && m.mtime_nsec() == before.mtime_ns
            })
        }) && AnchoredPath::open(self.path.parent().unwrap())
            .is_ok_and(|parent| parent.witness.nodes == before.nodes[..before.nodes.len() - 1])
            && std::fs::symlink_metadata(&self.path)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
        let contents_verified = self.contents.as_ref().is_none_or(|expected| {
            destination.as_ref().is_some_and(|path| {
                crate::tree::inspect_contents_held(self.held.target(), path, node.dev, node.ino)
                    .is_ok_and(|actual| &actual == expected)
            })
        });
        Ok(
            if verified
                && contents_verified
                && receipt
                    .as_ref()
                    .is_some_and(|r| r.device == node.dev && r.inode == node.ino)
            {
                Attempt {
                    outcome: Outcome::Moved(receipt.unwrap()),
                    reason: "已移入废纸篓，并核对实际目标身份与原路径范围。".into(),
                }
            } else {
                Attempt {
                    outcome: Outcome::Uncertain(receipt),
                    reason: format!("处理结果、内容或原路径范围发生变化；结果待核实，已停止后续项目。（对象核对：{verified}，目录内容核对：{contents_verified}）"),
                }
            },
        )
    }
}

#[cfg(test)]
mod tests;
