//! Optional desktop geometry. No file/task state, frontend command or permission.
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

const MIN_WIDTH: f64 = 960.0;
const MIN_HEIGHT: f64 = 680.0;
const MAX_EDGE: f64 = 8192.0;
const BODY_LIMIT: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    width: f64,
    height: f64,
    x: i32,
    y: i32,
    scale: f64,
}
impl Placement {
    fn valid(self) -> bool {
        self.width.is_finite()
            && self.height.is_finite()
            && valid_scale(self.scale)
            && (MIN_WIDTH..=MAX_EDGE).contains(&self.width)
            && (MIN_HEIGHT..=MAX_EDGE).contains(&self.height)
    }
}

#[derive(Clone, Copy, Debug)]
struct WorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
}
fn valid_scale(scale: f64) -> bool {
    scale.is_finite() && (0.5..=8.0).contains(&scale)
}

// Logical size avoids multiplying a Retina size on every launch. Positions are
// physical global coordinates and may legitimately be negative on another screen.
fn captured(
    size: (u32, u32),
    position: (i32, i32),
    scale: f64,
    ordinary: bool,
) -> Option<Placement> {
    if !ordinary || !valid_scale(scale) {
        return None;
    }
    let value = Placement {
        width: f64::from(size.0) / scale,
        height: f64::from(size.1) / scale,
        x: position.0,
        y: position.1,
        scale,
    };
    value.valid().then_some(value)
}

// Select the current work area containing the largest part of the saved window;
// if its screen disappeared use the primary work area (first), then clamp every
// edge. Frame allowance is read from the current native window, not assumed.
fn resolve(saved: Placement, areas: &[WorkArea], frame: (f64, f64)) -> Option<Placement> {
    if !saved.valid()
        || !frame.0.is_finite()
        || !frame.1.is_finite()
        || !(0.0..=256.0).contains(&frame.0)
        || !(0.0..=256.0).contains(&frame.1)
    {
        return None;
    }
    let eligible = areas.iter().filter(|area| {
        valid_scale(area.scale)
            && f64::from(area.width) / area.scale >= MIN_WIDTH + frame.0
            && f64::from(area.height) / area.scale >= MIN_HEIGHT + frame.1
    });
    let mut selected = None;
    let mut largest = -1.0;
    for area in eligible {
        let left = f64::from(saved.x).max(f64::from(area.x));
        let top = f64::from(saved.y).max(f64::from(area.y));
        let right = (f64::from(saved.x) + (saved.width + frame.0) * saved.scale)
            .min(f64::from(area.x) + f64::from(area.width));
        let bottom = (f64::from(saved.y) + (saved.height + frame.1) * saved.scale)
            .min(f64::from(area.y) + f64::from(area.height));
        let overlap = (right - left).max(0.0) * (bottom - top).max(0.0);
        if overlap > largest {
            selected = Some(area);
            largest = overlap;
        }
    }
    let area = selected?;
    // Round the available inner size down so physical rounding cannot push an
    // edge beyond the work area. Keep the configured minimum layout intact.
    let width = saved
        .width
        .min((f64::from(area.width) / area.scale - frame.0).floor());
    let height = saved
        .height
        .min((f64::from(area.height) / area.scale - frame.1).floor());
    let right = f64::from(area.x) + f64::from(area.width) - ((width + frame.0) * area.scale).ceil();
    let bottom =
        f64::from(area.y) + f64::from(area.height) - ((height + frame.1) * area.scale).ceil();
    let value = Placement {
        width,
        height,
        scale: area.scale,
        x: f64::from(saved.x)
            .clamp(f64::from(area.x), right)
            .floor()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
        y: f64::from(saved.y)
            .clamp(f64::from(area.y), bottom)
            .floor()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
    };
    value.valid().then_some(value)
}

pub fn load(db: &Connection) -> Result<Option<Placement>, String> {
    let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='window_preferences')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if !exists {
        return Ok(None);
    }
    let row = db.query_row("SELECT version, CASE WHEN typeof(body)='text' AND length(CAST(body AS BLOB)) <= 1024 THEN body END FROM window_preferences WHERE id=1", [], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, Option<String>>(1)?))).optional().map_err(|e| e.to_string())?;
    let Some((version, body)) = row else {
        return Ok(None);
    };
    if version != 1 {
        return Err("窗口偏好版本不支持，使用默认窗口。".into());
    }
    let body = body.ok_or("窗口偏好超限或格式无效。")?;
    let value: Placement = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if !value.valid() {
        return Err("窗口偏好尺寸无效。".into());
    }
    Ok(Some(value))
}

fn save(db: &Connection, value: Placement) -> Result<(), String> {
    if !value.valid() {
        return Err("窗口偏好尺寸无效。".into());
    }
    let body = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    if body.len() > BODY_LIMIT {
        return Err("窗口偏好超限。".into());
    }
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS window_preferences (id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL, body TEXT NOT NULL)").map_err(|e| e.to_string())?;
    let changed = tx.execute("INSERT INTO window_preferences VALUES (1,1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body WHERE window_preferences.version=1", [body]).map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("保留更新版本的窗口偏好。".into());
    }
    tx.commit().map_err(|e| e.to_string())
}

// Exit must not wait for another SQLite writer. Restore the connection's prior
// busy timeout while its application mutex is still held, even on save failure.
fn save_without_wait(db: &Connection, value: Placement) -> Result<(), String> {
    let previous: u64 = db
        .pragma_query_value(None, "busy_timeout", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::ZERO)
        .map_err(|e| e.to_string())?;
    let result = save(db, value);
    let restored = db
        .busy_timeout(std::time::Duration::from_millis(previous))
        .map_err(|e| e.to_string());
    result.and(restored)
}

#[derive(Default)]
pub struct Tracker {
    latest: Mutex<Option<Placement>>,
    flushed: AtomicBool,
}
impl Tracker {
    pub fn capture(&self, window: &tauri::Window) {
        let ordinary = window.is_visible().ok() == Some(true)
            && window.is_minimized().ok() == Some(false)
            && window.is_maximized().ok() == Some(false)
            && window.is_fullscreen().ok() == Some(false);
        if !ordinary {
            return;
        }
        if let (Ok(size), Ok(position), Ok(scale)) = (
            window.inner_size(),
            window.outer_position(),
            window.scale_factor(),
        ) {
            if let Some(value) = captured(
                (size.width, size.height),
                (position.x, position.y),
                scale,
                ordinary,
            ) {
                if let Ok(mut latest) = self.latest.lock() {
                    *latest = Some(value);
                }
            }
        }
    }

    pub fn restore(&self, window: &tauri::Window, saved: Option<Placement>) {
        let result = (|| -> tauri::Result<()> {
            if let Some(saved) = saved {
                let scale = window.scale_factor()?;
                if !valid_scale(scale) {
                    return Ok(());
                }
                let inner = window.inner_size()?;
                let outer = window.outer_size()?;
                let frame = (
                    f64::from(outer.width.saturating_sub(inner.width)) / scale,
                    f64::from(outer.height.saturating_sub(inner.height)) / scale,
                );
                let mut monitors = window.available_monitors()?;
                if let Some(primary) = window.primary_monitor()? {
                    monitors.sort_by_key(|m| {
                        (m.position() != primary.position()) || (m.size() != primary.size())
                    });
                }
                let areas: Vec<_> = monitors
                    .iter()
                    .map(|m| {
                        let work = m.work_area();
                        WorkArea {
                            x: work.position.x,
                            y: work.position.y,
                            width: work.size.width,
                            height: work.size.height,
                            scale: m.scale_factor(),
                        }
                    })
                    .collect();
                if let Some(value) = resolve(saved, &areas, frame) {
                    window.set_size(tauri::LogicalSize::new(value.width, value.height))?;
                    window.set_position(tauri::PhysicalPosition::new(value.x, value.y))?;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("窗口位置未能恢复，继续使用当前窗口：{error}");
        }
        self.capture(window);
    }

    pub fn flush(&self, database: &Mutex<crate::db::Database>) {
        if self.flushed.load(Ordering::SeqCst) {
            return;
        }
        let value = self.latest.lock().ok().and_then(|value| *value);
        let Some(value) = value else {
            return;
        };
        let Ok(db) = database.try_lock() else {
            return;
        };
        if self.flushed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Err(error) = save_without_wait(&db, value) {
            eprintln!("窗口偏好未能保存，保留此前记录：{error}");
        }
    }
}

#[cfg(test)]
mod tests;
