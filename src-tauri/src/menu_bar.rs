//! Explicitly enabled menu-bar observation. No helper, login registration or
//! file-processing action; Quit follows the application's existing drain.
use crate::{db, language, AppState};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    Emitter, Manager,
};
const ID: &str = "moomole-observation";
#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    enabled: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    config: Config,
    visible: bool,
    error: Option<String>,
}
#[derive(Default)]
pub(crate) struct Controller {
    visible: AtomicBool,
    transition: Mutex<()>,
    native: Mutex<Option<(TrayIcon, Model)>>,
    error: Mutex<Option<String>>,
}
#[derive(Clone, PartialEq, Debug)]
struct Model {
    title: String,
    reading: String,
    status: String,
    language: String,
    collecting: bool,
}
fn load(db: &rusqlite::Connection) -> Result<Config, String> {
    let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='menu_bar_preferences' AND type='table')", [], |r|r.get(0)).map_err(|e|e.to_string())?;
    if !exists {
        return Ok(Config::default());
    }
    let row: Option<(u32,Option<String>)> = db.query_row("SELECT version,CASE WHEN length(CAST(body AS BLOB))<=1024 THEN body END FROM menu_bar_preferences WHERE id=1",[],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
    let Some((version, body)) = row else {
        return Ok(Config::default());
    };
    if version != 1 {
        return Err("菜单栏设置版本不支持，未开启菜单栏。".into());
    }
    serde_json::from_str(&body.ok_or("菜单栏设置超过读取范围。")?)
        .map_err(|_| "菜单栏设置无法核对，未开启菜单栏。".into())
}
fn save(db: &mut rusqlite::Connection, config: Config) -> Result<(), String> {
    let _ = load(db)?;
    if !db::settings(db)?.acknowledged {
        return Err("请先确认首次使用说明。".into());
    }
    let t = db.transaction().map_err(|e| e.to_string())?;
    t.execute_batch("CREATE TABLE IF NOT EXISTS menu_bar_preferences(id INTEGER PRIMARY KEY CHECK(id=1),version INTEGER NOT NULL,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=1024));").map_err(|e|e.to_string())?;
    t.execute("INSERT INTO menu_bar_preferences VALUES(1,1,?1) ON CONFLICT(id) DO UPDATE SET version=1,body=excluded.body", [serde_json::to_string(&config).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    t.commit().map_err(|e| e.to_string())
}
fn model(view: &crate::storage_monitor::View, preference: &str, now: i64) -> Model {
    let t = |zh, en, ja| language::text(preference, zh, en, ja);
    let latest = view.samples.last();
    let fresh = latest.is_some_and(|s| {
        now >= s.measured_at
            && now.saturating_sub(s.measured_at)
                <= i64::from(view.config.interval_minutes) * 120_000
    }) && view.error.is_none();
    let low = fresh && latest.is_some_and(|s| s.available_bytes < view.config.warn_below_bytes);
    let value = latest.map(|s| format!("{:.1} GiB", s.available_bytes as f64 / 1024_f64.powi(3)));
    Model {
        title: value
            .as_ref()
            .filter(|_| fresh)
            .map(|v| format!("{}{v}", if low { "! " } else { "" }))
            .unwrap_or_default(),
        reading: value
            .map(|v| {
                format!(
                    "{} {v}",
                    t(
                        "账户所在卷可用",
                        "Account volume available",
                        "アカウントのボリューム空き容量"
                    )
                )
            })
            .unwrap_or_else(|| {
                t(
                    "可用空间尚未记录",
                    "Available space not recorded",
                    "空き容量の記録なし",
                )
                .into()
            }),
        status: if view.error.is_some() {
            t(
                "采集失败，请在持续管理查看",
                "Collection failed; check ongoing management",
                "取得失敗：継続管理で確認してください",
            )
        } else if low {
            t(
                "最近观察低于空间阈值",
                "Recent observation below space threshold",
                "直近の空き容量がしきい値未満です",
            )
        } else if fresh {
            t(
                "最近观察；容量并非实时值",
                "Recent observation; capacity is not live",
                "直近の記録：容量はリアルタイムではありません",
            )
        } else {
            t(
                "历史观察，当前状态未知",
                "Historical observation; current state unknown",
                "過去の記録：現在の状態は不明です",
            )
        }
        .into(),
        language: preference.into(),
        collecting: view.collecting,
    }
}
fn menu(app: &tauri::AppHandle, m: &Model) -> tauri::Result<Menu<tauri::Wry>> {
    let t = |zh, en, ja| language::text(&m.language, zh, en, ja);
    let reading = MenuItem::new(app, &m.reading, false, None::<&str>)?;
    let status = MenuItem::new(app, &m.status, false, None::<&str>)?;
    let open = MenuItem::with_id(
        app,
        "observation-open",
        t("打开 MooMole", "Open MooMole", "MooMole を開く"),
        true,
        None::<&str>,
    )?;
    let trends = MenuItem::with_id(
        app,
        "observation-trends",
        t(
            "查看空间趋势与设置",
            "Space trends and settings",
            "空き容量の推移と設定",
        ),
        true,
        None::<&str>,
    )?;
    let sample = MenuItem::with_id(
        app,
        "observation-sample",
        if m.collecting {
            t("正在记录…", "Recording…", "記録中…")
        } else {
            t(
                "记录当前空间",
                "Record current space",
                "現在の空き容量を記録",
            )
        },
        !m.collecting,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(
        app,
        "observation-quit",
        t("退出 MooMole", "Quit MooMole", "MooMole を終了"),
        true,
        None::<&str>,
    )?;
    Menu::with_items(
        app,
        &[&reading, &status, &sep, &open, &trends, &sample, &quit],
    )
}
// A monochrome template mark, drawn as native pixels and scaled by macOS.
fn icon() -> tauri::image::Image<'static> {
    let mut rgba = vec![0; 18 * 18 * 4];
    for y in 4..15 {
        for x in 3..15 {
            if (3..5).contains(&x)
                || (13..15).contains(&x)
                || ((8..10).contains(&x) && y >= 8)
                || (y < 9
                    && ((x as i32 - y as i32).abs() < 2 || (17 - x as i32 - y as i32).abs() < 2))
            {
                rgba[(y * 18 + x) * 4 + 3] = 255;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, 18, 18)
}
impl Controller {
    pub(crate) fn keeps_running(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
    fn apply(&self, app: &tauri::AppHandle, enabled: bool, m: Option<Model>) -> Result<(), String> {
        let mut native = self.native.lock().map_err(|e| e.to_string())?;
        if !enabled {
            self.visible.store(false, Ordering::SeqCst);
            drop(app.remove_tray_by_id(ID));
            *native = None;
            return Ok(());
        }
        let m = m.ok_or("菜单栏容量状态不可读取。")?;
        if let Some((tray, old)) = native.as_mut() {
            if *old != m {
                tray.set_menu(Some(menu(app, &m).map_err(|e| e.to_string())?))
                    .map_err(|e| e.to_string())?;
                tray.set_title(Some(&m.title)).map_err(|e| e.to_string())?;
                tray.set_tooltip(Some(format!("MooMole · {} · {}", m.reading, m.status)))
                    .map_err(|e| e.to_string())?;
                *old = m;
            }
        } else {
            let tray = TrayIconBuilder::with_id(ID)
                .icon(icon())
                .icon_as_template(true)
                .title(&m.title)
                .tooltip(format!("MooMole · {} · {}", m.reading, m.status))
                .menu(&menu(app, &m).map_err(|e| e.to_string())?)
                .build(app)
                .map_err(|e| e.to_string())?;
            *native = Some((tray, m));
        }
        self.visible.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn refresh(&self, app: &tauri::AppHandle, state: &AppState) -> Result<(), String> {
        let _transition = self.transition.lock().map_err(|e| e.to_string())?;
        self.refresh_locked(app, state)
    }
    fn refresh_locked(&self, app: &tauri::AppHandle, state: &AppState) -> Result<(), String> {
        let read = (|| -> Result<_, String> {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let config = load(&db)?;
            let m = if config.enabled && db::settings(&db)?.acknowledged {
                Some(model(
                    &state.storage_monitor.view(&db)?,
                    &db::settings(&db)?.language,
                    db::now(),
                ))
            } else {
                None
            };
            Ok((config, m))
        })();
        match read {
            Ok((config, m)) => self.apply(app, config.enabled && m.is_some(), m),
            Err(error) => {
                let previous = self
                    .native
                    .lock()
                    .map_err(|e| e.to_string())?
                    .as_ref()
                    .map(|(_, m)| m.clone());
                if let Some(mut m) = previous {
                    m.title.clear();
                    m.reading = language::text(
                        &m.language,
                        "容量状态未知",
                        "Capacity state unknown",
                        "容量の状態は不明です",
                    )
                    .into();
                    m.status = language::text(
                        &m.language,
                        "读取失败，请在持续管理查看",
                        "Read failed; check ongoing management",
                        "読み取り失敗：継続管理で確認してください",
                    )
                    .into();
                    let _ = self.apply(app, true, Some(m));
                }
                Err(error)
            }
        }
    }
    fn view(&self, state: &AppState) -> Result<View, String> {
        Ok(View {
            config: load(&*state.db.lock().map_err(|e| e.to_string())?)?,
            visible: self.keeps_running(),
            error: self.error.lock().map_err(|e| e.to_string())?.clone(),
        })
    }
}
pub(crate) fn start(app: &tauri::AppHandle) {
    app.manage(Controller::default());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let state = app.state::<AppState>();
            if state.shutdown.closing() {
                break;
            }
            let controller = app.state::<Controller>();
            let result = controller.refresh(&app, &state);
            if let Ok(mut error) = controller.error.lock() {
                *error = result.err();
            }
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}
pub(crate) fn show(app: &tauri::AppHandle, page: Option<&str>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(page) = page {
            let _ = window.emit("moomole-menu-bar", page);
        }
    }
}
pub(crate) fn event(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    if !app
        .try_state::<Controller>()
        .is_some_and(|c| c.keeps_running())
    {
        return;
    }
    match event.id.as_ref() {
        "observation-open" => show(app, None),
        "observation-trends" => show(app, Some("storageMonitor")),
        "observation-quit" => app.exit(0),
        "observation-sample" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                if !state.shutdown.closing() {
                    let _ = state
                        .storage_monitor
                        .collect(&state.db, state.home.clone(), None)
                        .await;
                    let controller = app.state::<Controller>();
                    let result = controller.refresh(&app, &state);
                    if let Ok(mut error) = controller.error.lock() {
                        *error = result.err();
                    };
                }
            });
        }
        _ => {}
    }
}
#[tauri::command]
pub(crate) fn get_menu_bar(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<View, String> {
    app.state::<Controller>().view(&state)
}
#[tauri::command]
pub(crate) async fn configure_menu_bar(
    config: Config,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<View, String> {
    if state.shutdown.closing() {
        return Err("应用正在退出。".into());
    }
    let _operation = state.updates.installation.enter()?;
    // Database failure leaves current native state unchanged; a native failure
    // is reported with the saved preference and can be retried explicitly.
    let controller = app.state::<Controller>();
    let _transition = controller.transition.lock().map_err(|e| e.to_string())?;
    save(&mut *state.db.lock().map_err(|e| e.to_string())?, config)?;
    let result = controller.refresh_locked(&app, &state);
    *controller.error.lock().map_err(|e| e.to_string())? = result.err();
    if !config.enabled {
        show(&app, None);
    }
    controller.view(&state)
}
#[cfg(test)]
mod tests;
