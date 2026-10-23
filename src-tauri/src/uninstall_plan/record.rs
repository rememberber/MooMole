//! Historical evidence, deliberately NOT Plan and never deserialized into one.
use super::*;
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Archive {
    version: u32,
    operation: String,
    scope: String,
    related_data: String,
    plan_id: String,
    source: Source,
    snapshot: identity::Snapshot,
    home: PathBuf,
    uid: u32,
    created_at: i64,
    expires_at: i64,
    revision: String,
    digest: String,
}

impl Plan {
    pub(crate) fn archive(&self) -> Archive {
        Archive {
            version: 1,
            operation: "appBundleTrash".into(),
            scope: "appBundleOnly".into(),
            related_data: "retainAll".into(),
            plan_id: self.id.clone(),
            source: self.source.clone(),
            snapshot: self.snapshot.clone(),
            home: self.home.clone(),
            uid: self.uid,
            created_at: self.created_at,
            expires_at: self.expires_at,
            revision: self.revision.clone(),
            digest: self.digest.clone(),
        }
    }
}

pub(crate) fn absolute(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains('\0')
        && path.is_absolute()
        && path.parent().is_some()
        && !value
            .as_bytes()
            .split(|b| *b == b'/')
            .any(|p| p == b"." || p == b"..")
}
pub(crate) fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Archive {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let path = Path::new(&self.source.path);
        let source_row = serde_json::json!({"path":self.source.path,"bundle_id":self.source.bundle_id,"name":self.source.name,"source":self.source.source});
        let source = Source::from_row(&self.source.task_id, self.source.index, &source_row)?;
        if self.version != 1
            || self.operation != "appBundleTrash"
            || self.scope != "appBundleOnly"
            || self.related_data != "retainAll"
            || uuid::Uuid::parse_str(&self.plan_id).is_err()
            || source != self.source
            || self.source.source != "App"
            || !absolute(&self.source.path)
            || path.extension().is_none_or(|ext| ext != "app")
            || self.home.to_str().is_none_or(|home| !absolute(home))
            || (path.parent() != Some(Path::new("/Applications"))
                && path.parent() != Some(self.home.join("Applications").as_path()))
            || self.uid == 0
            || self.created_at < 0
            || self.expires_at.checked_sub(self.created_at) != Some(LIFETIME_MS)
            || self.revision.is_empty()
            || self.revision.len() > 128
            || self.revision.chars().any(char::is_control)
            || self.snapshot.version != 1
            || self.snapshot.bundle_id != self.source.bundle_id
            || !hash(&self.snapshot.info_sha256)
            || self.snapshot.inventory.version != 1
            || !hash(&self.snapshot.inventory.digest)
            || self.snapshot.inventory.entries > crate::tree::MAX_ENTRIES
            || !matches!(self.snapshot.background, Marker::Absent | Marker::Disabled)
            || digest_at_revision(&self.revision, &self.source, &self.snapshot)? != self.digest
        {
            return Err("卸载快照的范围、版本或身份摘要不一致；保留记录供核查。".into());
        }
        // Four typed witnesses must describe the expected exact subtree. No
        // filesystem access or fresh eligibility is inferred from old metadata.
        for (w, expected, kind) in [
            (&self.snapshot.root, path.to_owned(), libc::S_IFDIR),
            (
                &self.snapshot.contents,
                path.join("Contents"),
                libc::S_IFDIR,
            ),
            (
                &self.snapshot.info,
                path.join("Contents/Info.plist"),
                libc::S_IFREG,
            ),
            (
                &self.snapshot.executable,
                path.join("Contents/MacOS")
                    .join(&self.snapshot.executable_name),
                libc::S_IFREG,
            ),
        ] {
            if w.version != crate::observation::VERSION
                || w.nodes.is_empty()
                || w.nodes.len() > 129
                || w.links == 0
                || w.nodes.last().is_none_or(|n| {
                    n.path != expected || n.mode & libc::S_IFMT as u32 != kind as u32
                })
                || w.nodes.first().is_none_or(|n| n.path != Path::new("/"))
                || w.nodes
                    .windows(2)
                    .any(|n| n[1].path.parent() != Some(n[0].path.as_path()))
                || w.nodes[..w.nodes.len() - 1]
                    .iter()
                    .any(|n| n.mode & libc::S_IFMT as u32 != libc::S_IFDIR as u32)
                || !w.nodes.starts_with(&self.snapshot.root.nodes)
                || (kind == libc::S_IFREG && w.links != 1)
            {
                return Err("卸载快照缺少完整对象或祖先身份；保留记录供核查。".into());
            }
        }
        if self.snapshot.executable_name.is_empty()
            || self.snapshot.executable_name.len() > 255
            || self.snapshot.executable_name.chars().any(char::is_control)
            || self.snapshot.executable_name == "."
            || self.snapshot.executable_name == ".."
            || Path::new(&self.snapshot.executable_name)
                .components()
                .count()
                != 1
            || self.snapshot.executable_name.contains('/')
        {
            return Err("卸载快照的主程序名称无效。".into());
        }
        Ok(())
    }
    pub(crate) fn live_at(&self, at: i64) -> Result<(), String> {
        self.validate()?;
        if self.revision != engine::REVISION
            || self.uid != unsafe { libc::geteuid() }
            || !self.contains_time(at)
        {
            return Err("此卸载计划已过期或版本/账户不匹配，不能创建或继续操作意图。".into());
        }
        Ok(())
    }
    pub(crate) fn contains_time(&self, at: i64) -> bool {
        at >= self.created_at && at < self.expires_at
    }
    pub(crate) fn id(&self) -> &str {
        &self.plan_id
    }
    pub(crate) fn path(&self) -> &str {
        &self.source.path
    }
    pub(crate) fn name(&self) -> &str {
        &self.source.name
    }
    pub(crate) fn bundle_id(&self) -> &str {
        &self.source.bundle_id
    }
    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }
    pub(crate) fn snapshot(&self) -> &identity::Snapshot {
        &self.snapshot
    }
}
