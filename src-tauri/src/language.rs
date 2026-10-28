//! UI language only. Never translate structured execution/validation data.

pub(crate) fn resolve(preference: &str, system: &str) -> &'static str {
    let primary = if preference == "system" {
        system.split(['-', '_']).next().unwrap_or("")
    } else {
        preference
    };
    match primary.to_ascii_lowercase().as_str() {
        "zh" => "zh",
        "ja" => "ja",
        _ => "en",
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn system_language() -> String {
    use core_foundation_sys::{
        array::{CFArrayGetCount, CFArrayGetValueAtIndex},
        base::{CFGetTypeID, CFRelease},
        locale::CFLocaleCopyPreferredLanguages,
        string::{kCFStringEncodingUTF8, CFStringGetCString, CFStringGetTypeID},
    };
    // Copy API ownership is released on every path. CF strings are checked for
    // type and converted into a bounded UTF-8 buffer before leaving the scope.
    unsafe {
        let languages = CFLocaleCopyPreferredLanguages();
        if languages.is_null() {
            return "en".into();
        }
        let mut result = None;
        if CFArrayGetCount(languages) > 0 {
            let first = CFArrayGetValueAtIndex(languages, 0);
            if !first.is_null() && CFGetTypeID(first) == CFStringGetTypeID() {
                let mut buffer = [0i8; 256];
                if CFStringGetCString(
                    first.cast(),
                    buffer.as_mut_ptr(),
                    buffer.len() as isize,
                    kCFStringEncodingUTF8,
                ) != 0
                {
                    result = std::ffi::CStr::from_ptr(buffer.as_ptr())
                        .to_str()
                        .ok()
                        .map(str::to_owned);
                }
            }
        }
        CFRelease(languages.cast());
        result.unwrap_or_else(|| "en".into())
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn system_language() -> String {
    "en".into()
}

pub(crate) fn saved(state: &crate::AppState) -> String {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| crate::db::settings(&db).ok())
        .map(|settings| settings.language)
        .unwrap_or_else(|| "system".into())
}

pub(crate) fn text<'a>(preference: &str, zh: &'a str, en: &'a str, ja: &'a str) -> &'a str {
    match resolve(preference, &system_language()) {
        "zh" => zh,
        "ja" => ja,
        _ => en,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_reads_do_not_write_and_old_style_settings_writes_keep_language() {
        let temp = tempfile::tempdir().unwrap();
        let db = crate::db::open_fixture(&temp.path().join("settings.sqlite")).unwrap();
        assert_eq!(load(&db).unwrap(), "system");
        assert_eq!(
            db.query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_master WHERE name='language_preferences'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
        let settings = crate::db::Settings {
            language: "ja".into(),
            ..crate::db::Settings::default()
        };
        crate::db::save_settings(&db, &settings).unwrap();
        let json: String = db
            .query_row("SELECT value FROM settings WHERE id=1", [], |r| r.get(0))
            .unwrap();
        assert!(!json.contains("language"));
        db.execute(
            "UPDATE settings SET value=?1 WHERE id=1",
            [json.replace("system", "dark")],
        )
        .unwrap();
        assert_eq!(crate::db::settings(&db).unwrap().language, "ja");
        assert_eq!(crate::db::settings(&db).unwrap().theme, "dark");
        // A later failure rolls back the language and four-field JSON together.
        db.execute_batch("CREATE TRIGGER reject_settings BEFORE UPDATE ON settings BEGIN SELECT RAISE(ABORT, 'synthetic write failure'); END;").unwrap();
        let changed = crate::db::Settings {
            language: "en".into(),
            ..settings
        };
        assert!(crate::db::save_settings(&db, &changed).is_err());
        assert_eq!(load(&db).unwrap(), "ja");
    }

    #[test]
    fn primary_system_language_and_english_fallback() {
        for (system, expected) in [
            ("zh-Hans-CN", "zh"),
            ("zh-Hant-TW", "zh"),
            ("ja_JP", "ja"),
            ("en-GB", "en"),
            ("fr-FR", "en"),
            ("", "en"),
        ] {
            assert_eq!(resolve("system", system), expected);
        }
        assert_eq!(resolve("ja", "zh-CN"), "ja");
        assert!(!system_language().is_empty());
    }
}

// Optional additive storage, matching update_preferences. Reading an old
// database does not create a table or rewrite its strict settings JSON.
pub(crate) fn load(db: &rusqlite::Connection) -> Result<String, String> {
    use rusqlite::OptionalExtension;
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='language_preferences')",
        [], |row| row.get(0),
    ).map_err(|_| "无法读取语言设置。")?;
    if !exists {
        return Ok("system".into());
    }
    let value: Option<String> = db
        .query_row(
            "SELECT language FROM language_preferences WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| "无法读取语言设置。")?;
    match value {
        None => Ok("system".into()),
        Some(value) if ["system", "zh", "en", "ja"].contains(&value.as_str()) => Ok(value),
        _ => Err("语言设置无效。".into()),
    }
}
pub(crate) fn save(transaction: &rusqlite::Transaction<'_>, language: &str) -> Result<(), String> {
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS language_preferences (id INTEGER PRIMARY KEY CHECK(id=1), language TEXT NOT NULL CHECK(language IN ('system','zh','en','ja')))")
        .map_err(|_| "无法保存语言设置。")?;
    transaction.execute("INSERT INTO language_preferences VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET language=excluded.language", [language])
        .map_err(|_| "无法保存语言设置。")?;
    Ok(())
}

// Preserve Tauri's standard menu roles, accelerators, metadata and submenu IDs.
// Only the labels change. Default menus are recreated from Tauri's fixed menu
// model, never by interpreting user-provided text or command output.
#[cfg(target_os = "macos")]
pub(crate) fn apply_menu(app: &tauri::AppHandle, preference: &str) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItemKind};
    let language = resolve(preference, &system_language());
    fn label(source: &str, language: &str) -> String {
        if language == "en" {
            return source.to_owned();
        }
        let original = source.replace('&', "");
        for (en, zh, ja) in [
            ("File", "文件", "ファイル"),
            ("Edit", "编辑", "編集"),
            ("View", "显示", "表示"),
            ("Window", "窗口", "ウインドウ"),
            ("Help", "帮助", "ヘルプ"),
            ("Services", "服务", "サービス"),
            ("Undo", "撤销", "取り消す"),
            ("Redo", "重做", "やり直す"),
            ("Cut", "剪切", "カット"),
            ("Copy", "复制", "コピー"),
            ("Paste", "粘贴", "ペースト"),
            ("Select All", "全选", "すべてを選択"),
            ("Minimize", "最小化", "しまう"),
            ("Zoom", "缩放", "拡大／縮小"),
            ("Close Window", "关闭窗口", "ウインドウを閉じる"),
            (
                "Toggle Full Screen",
                "切换全屏",
                "フルスクリーンを切り替える",
            ),
            ("Hide Others", "隐藏其他", "ほかを非表示"),
        ] {
            if original == en {
                return if language == "zh" { zh } else { ja }.into();
            }
        }
        for (verb, zh, ja) in [
            ("About ", "关于 ", " について"),
            ("Hide ", "隐藏 ", " を非表示"),
            ("Quit ", "退出 ", " を終了"),
        ] {
            if let Some(name) = original.strip_prefix(verb) {
                return if language == "zh" {
                    format!("{zh}{name}")
                } else {
                    format!("{name}{ja}")
                };
            }
        }
        source.to_owned()
    }
    fn localize(items: Vec<MenuItemKind<tauri::Wry>>, language: &str) -> tauri::Result<()> {
        for item in items {
            match item {
                MenuItemKind::Submenu(menu) => {
                    menu.set_text(label(&menu.text()?, language))?;
                    localize(menu.items()?, language)?;
                }
                MenuItemKind::Predefined(item) => item.set_text(label(&item.text()?, language))?,
                _ => {}
            }
        }
        Ok(())
    }
    let menu = Menu::default(app)?;
    localize(menu.items()?, language)?;
    app.set_menu(menu)?;
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn apply_menu(_app: &tauri::AppHandle, _preference: &str) -> tauri::Result<()> {
    Ok(())
}
