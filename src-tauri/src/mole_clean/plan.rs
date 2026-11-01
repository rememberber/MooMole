//! An explicit whole-command plan. Never substitutes for a selected-file plan.
use crate::{db, engine, engine_manifest, AppState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

pub const LIFETIME: i64 = 120_000;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub version: u32,
    pub id: String,
    pub revision: String,
    pub engine_version: String,
    pub home: String,
    pub system: bool,
    pub method: String,
    pub protected_paths: Vec<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub scope: Vec<String>,
}
pub struct Authorized {
    pub view: Plan,
    pub engine: PathBuf,
    pub binding: String,
    pub issued: Instant,
}
fn binding(state: &AppState, verify: bool) -> Result<String, String> {
    let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取清理保护。")?)?;
    if !settings.acknowledged || state.shutdown.closing() {
        return Err("请先确认使用说明，并等待应用完成当前操作。".into());
    }
    if verify {
        engine_manifest::verify(&state.engine_root)?;
    }
    let home = std::fs::metadata(&state.home).map_err(|e| e.to_string())?;
    use std::os::unix::fs::MetadataExt;
    let whitelist = state.home.join(".config/mole/whitelist");
    let policy = match std::fs::symlink_metadata(&whitelist) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() <= 1024 * 1024 => {
            std::fs::read(&whitelist).map_err(|e| e.to_string())?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        _ => return Err("Mole 白名单无法核对，未准备完整清理。".into()),
    };
    let fingerprint =
        std::fs::read(state.engine_root.join("UPSTREAM.json")).map_err(|e| e.to_string())?;
    Ok(format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                engine::REVISION,
                home.dev(),
                home.ino(),
                settings.protected_paths,
                policy,
                fingerprint
            ))
            .map_err(|e| e.to_string())?
        )
    ))
}
impl Authorized {
    pub fn prepare(state: &AppState, system: bool) -> Result<Self, String> {
        if unsafe { libc::geteuid() } == 0 {
            return Err("完整 Mole 清理不能以 root App 运行，请使用普通账户。".into());
        }
        let binding = binding(state, true)?;
        let manifest = engine_manifest::verify(&state.engine_root)?;
        let settings = db::settings(&*state.db.lock().map_err(|_| "无法读取清理保护。")?)?;
        let now = db::now();
        Ok(Self {
            view: Plan {
                version: 1,
                id: uuid::Uuid::new_v4().to_string(),
                revision: engine::REVISION.into(),
                engine_version: manifest.version,
                home: state.home.to_str().ok_or("账户目录无法编码。")?.into(),
                system,
                method: "molePermanent".into(),
                protected_paths: settings.protected_paths,
                created_at: now,
                expires_at: now + LIFETIME,
                scope: [
                    "system",
                    "userEssentials",
                    "appCaches",
                    "browsers",
                    "cloudOffice",
                    "developerTools",
                    "appsUtilities",
                    "virtualization",
                    "applicationSupport",
                    "appLeftovers",
                    "appleSilicon",
                    "firmware",
                    "timeMachine",
                    "largeFiles",
                    "projectArtifacts",
                ]
                .map(String::from)
                .to_vec(),
            },
            engine: state.engine_root.clone(),
            binding,
            issued: Instant::now(),
        })
    }
    pub fn validate(&self, state: &AppState) -> Result<(), String> {
        let now = db::now();
        if now < self.view.created_at
            || now >= self.view.expires_at
            || self.issued.elapsed().as_millis() >= LIFETIME as u128
            || binding(state, true)? != self.binding
        {
            return Err(
                "完整清理确认已过期，或引擎、账户、保护与白名单已变化，请重新确认。".into(),
            );
        }
        Ok(())
    }
    pub fn validate_running(&self, state: &AppState) -> Result<(), String> {
        // Confirmation TTL is admission-only; protection and engine binding continue to apply.
        if binding(state, false)? != self.binding {
            return Err("清理保护或引擎已变化，已停止后续操作。".into());
        }
        Ok(())
    }
}
