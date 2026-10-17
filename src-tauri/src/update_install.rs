//! Admission barrier for manual installation; never replaces the running App.
use std::sync::{Arc, Mutex};
use std::{path::Path, process::Stdio, time::Duration};

#[derive(Default)]
pub struct Gate(Mutex<State>);
#[derive(Default)]
struct State {
    operations: usize,
    installing: bool,
}
pub struct Operation {
    gate: Arc<Gate>,
}
pub struct Installation {
    gate: Arc<Gate>,
    release: bool,
}
impl Gate {
    pub fn enter(self: &Arc<Self>) -> Result<Operation, String> {
        let mut s = self.0.lock().map_err(|_| "无法确认安装状态。")?;
        if s.installing {
            return Err("正在准备安装更新，请稍候。".into());
        }
        s.operations += 1;
        Ok(Operation { gate: self.clone() })
    }
    pub fn begin(self: &Arc<Self>) -> Result<Installation, String> {
        let mut s = self.0.lock().map_err(|_| "无法确认安装状态。")?;
        if s.installing {
            return Err("正在准备安装更新，请勿重复操作。".into());
        }
        if s.operations != 0 {
            return Err("还有检查或数据保存正在进行，请等待完成或先取消任务，再安装。".into());
        }
        s.installing = true;
        Ok(Installation {
            gate: self.clone(),
            release: true,
        })
    }
    pub fn active(&self) -> bool {
        self.0.lock().map(|s| s.installing).unwrap_or(true)
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.gate
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .operations -= 1;
    }
}
impl Installation {
    /// Keep admission closed after a successful open until the process exits.
    pub fn commit(mut self) {
        self.release = false;
    }
}
impl Drop for Installation {
    fn drop(&mut self) {
        if self.release {
            self.gate
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .installing = false;
        }
    }
}

pub fn prepare_data(db: &rusqlite::Connection) -> Result<(), String> {
    let pending: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE state IN ('queued','running')) OR EXISTS(SELECT 1 FROM execution_runs WHERE state='running')",
        [], |row| row.get(0),
    ).map_err(|_| "无法确认本机任务记录，请稍后重试。")?;
    if pending || !db.is_autocommit() {
        return Err(
            "还有未完成的任务记录，请完成检查后再安装；记录异常时请先重新启动应用。".into(),
        );
    }
    let busy: i64 = db
        .query_row("PRAGMA wal_checkpoint(FULL)", [], |r| r.get(0))
        .map_err(|_| "本机数据保存检查未完成，请稍后重试。")?;
    if busy != 0 {
        return Err("本机数据仍被占用，请稍后重试。".into());
    }
    Ok(())
}

fn command(path: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("/usr/bin/open");
    command
        .args([
            "-a",
            "/System/Library/CoreServices/DiskImageMounter.app",
            "--",
        ])
        .arg(path);
    command
}
async fn request(mut command: tokio::process::Command, limit: Duration) -> Result<(), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(limit, command.status())
        .await
        .map_err(|_| "打开安装包超时，应用保持运行；可重试或在 Finder 中查看。")?
        .map_err(|_| "无法打开安装包，应用保持运行；请重试。")?;
    if !status.success() {
        return Err("系统未接受打开安装包，应用保持运行；可重试或在 Finder 中查看。".into());
    }
    Ok(())
}
pub async fn open_dmg(path: std::path::PathBuf) -> Result<(), String> {
    request(command(&path), Duration::from_secs(5)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_is_exclusive_and_drop_reopens_after_failure_or_unwind() {
        let gate = Arc::new(Gate::default());
        let one = gate.enter().unwrap();
        let two = gate.enter().unwrap();
        assert!(gate.begin().is_err());
        drop(one);
        assert!(gate.begin().is_err());
        drop(two);
        let installation = gate.begin().unwrap();
        assert!(gate.active());
        assert!(gate.enter().is_err());
        assert!(gate.begin().is_err());
        drop(installation);
        let _ = std::panic::catch_unwind(|| {
            let _guard = gate.begin().unwrap();
            panic!("fixture");
        });
        assert!(!gate.active());
        assert!(gate.enter().is_ok());
    }
    #[test]
    fn data_barrier_refuses_unfinished_records_and_transaction_and_preserves_settings() {
        let temp = tempfile::tempdir().unwrap();
        let db = crate::db::open_fixture(&temp.path().join("db.sqlite")).unwrap();
        crate::db::save_settings(&db, &crate::db::Settings::default()).unwrap();
        prepare_data(&db).unwrap();
        crate::db::start(&db, "fixture", "status", None).unwrap();
        assert!(prepare_data(&db).is_err());
        crate::db::finish(&db, "fixture", "succeeded", "fixture").unwrap();
        db.execute_batch("BEGIN").unwrap();
        assert!(prepare_data(&db).is_err());
        db.execute_batch("ROLLBACK").unwrap();
        prepare_data(&db).unwrap();
        assert_eq!(crate::db::settings(&db).unwrap().theme, "system");
        // A persisted modification run is also a blocker, independently of jobs.
        db.execute("INSERT INTO execution_runs VALUES ('fixture','plan','trash','running',1,NULL,'{}','fixture',0)", []).unwrap();
        assert!(prepare_data(&db).is_err());
    }
    #[test]
    fn data_checkpoint_busy_retains_committed_settings_and_allows_retry() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db.sqlite");
        let db = crate::db::open_fixture(&path).unwrap();
        db.busy_timeout(Duration::from_millis(20)).unwrap();
        crate::db::save_settings(&db, &crate::db::Settings::default()).unwrap();
        prepare_data(&db).unwrap();
        let reader = rusqlite::Connection::open(&path).unwrap();
        reader
            .execute_batch("BEGIN; SELECT * FROM settings;")
            .unwrap();
        let mut settings = crate::db::Settings::default();
        settings.theme = "dark".into();
        crate::db::save_settings(&db, &settings).unwrap();
        assert!(prepare_data(&db).unwrap_err().contains("占用"));
        assert_eq!(crate::db::settings(&db).unwrap().theme, "dark");
        reader.execute_batch("ROLLBACK").unwrap();
        prepare_data(&db).unwrap();
        assert_eq!(crate::db::settings(&reader).unwrap().theme, "dark");
    }
    #[tokio::test]
    async fn system_request_uses_fixed_mounter_and_preserves_errors_and_timeout() {
        let path = Path::new("/private/cache/含 空格/installer.dmg");
        let c = command(path);
        assert_eq!(c.as_std().get_program(), "/usr/bin/open");
        assert_eq!(
            c.as_std().get_args().collect::<Vec<_>>(),
            vec![
                "-a",
                "/System/Library/CoreServices/DiskImageMounter.app",
                "--",
                path.to_str().unwrap()
            ]
        );
        assert!(request(
            tokio::process::Command::new("/usr/bin/true"),
            Duration::from_secs(1)
        )
        .await
        .is_ok());
        assert!(request(
            tokio::process::Command::new("/usr/bin/false"),
            Duration::from_secs(1)
        )
        .await
        .is_err());
        assert!(request(
            tokio::process::Command::new("/nonexistent/MooMole-test"),
            Duration::from_secs(1)
        )
        .await
        .is_err());
        let mut sleep = tokio::process::Command::new("/bin/sleep");
        sleep.arg("5");
        assert!(request(sleep, Duration::from_millis(20))
            .await
            .unwrap_err()
            .contains("超时"));
    }
}
