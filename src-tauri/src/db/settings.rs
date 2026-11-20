//! Settings rows: the settings screen, bookmarks, the pairing code, and small state values.
use super::*;

pub(crate) fn settings(connection: &Connection) -> Result<Settings, String> {
    let threshold = setting(connection, "confidence_threshold")?
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.6)
        .clamp(0.0, 1.0);
    let (background_mode, launch_at_login) = crate::presence::normalize_presence(
        setting(connection, "background_mode")?.as_deref() == Some("1"),
        setting(connection, "launch_at_login")?.as_deref() == Some("1"),
    );
    let identity = crate::sync_meta::device_identity(connection)?;
    Ok(Settings {
        confidence_threshold: threshold,
        library_folder: setting(connection, "library_folder")?.unwrap_or_default(),
        builtin_browser: setting(connection, "builtin_browser")?.as_deref() == Some("1"),
        browser_force_dark: setting(connection, "browser_force_dark")?.as_deref() == Some("1"),
        browser_auto_register: setting(connection, "browser_auto_register")?.as_deref()
            != Some("0"),
        booth_link_target: normalize_booth_link_target(
            &setting(connection, "booth_link_target")?.unwrap_or_else(|| "external".into()),
        ),
        remote_library_folder: setting(connection, "remote_library_folder")?.unwrap_or_default(),
        remote_auto_sync: setting(connection, "remote_auto_sync")?.as_deref() == Some("1"),
        route_downloads: setting(connection, "route_downloads")?.as_deref() == Some("1"),
        background_supported: crate::presence::background_supported(),
        background_mode,
        launch_at_login,
        classification_dismissed_version: setting(connection, "classification_dismissed_version")?
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(0),
        ui_rail_width: crate::model::clamp_ui_rail_width(
            setting(connection, "ui_rail_width")?
                .and_then(|value| value.parse().ok())
                .unwrap_or(232),
        ),
        ui_rail_collapsed: setting(connection, "ui_rail_collapsed")?.as_deref() == Some("1"),
        ui_genre_width: crate::model::clamp_ui_genre_width(
            setting(connection, "ui_genre_width")?
                .and_then(|value| value.parse().ok())
                .unwrap_or(200),
        ),
        ui_genre_collapsed: setting(connection, "ui_genre_collapsed")?.as_deref() == Some("1"),
        preview_pane: setting(connection, "preview_pane")?.as_deref() == Some("1"),
        preview_pane_width: crate::model::clamp_preview_pane_width(
            setting(connection, "preview_pane_width")?
                .and_then(|value| value.parse().ok())
                .unwrap_or(300),
        ),
        avatar_hero: crate::model::normalize_avatar_hero(
            &setting(connection, "avatar_hero")?
                .unwrap_or_else(|| crate::model::DEFAULT_AVATAR_HERO.into()),
        ),
        right_panel_tabs: crate::model::normalize_right_panel_tabs(
            &setting(connection, "right_panel_tabs")?.unwrap_or_default(),
        ),
        check_update_on_startup: setting(connection, "check_update_on_startup")?.as_deref()
            == Some("1"),
        ui_update_dismissed_version: setting(connection, "ui_update_dismissed_version")?
            .unwrap_or_default(),
        device_id: identity.id,
        device_name: identity.name,
    })
}

pub(super) fn normalize_booth_link_target(value: &str) -> String {
    if value.trim() == "builtin" {
        "builtin".into()
    } else {
        "external".into()
    }
}

pub(crate) fn save_settings(connection: &Connection, input: &SettingsInput) -> Result<(), String> {
    save_settings_values(connection, input, true, None)
}

pub(crate) fn update_settings(
    connection: &Connection,
    patch: &SettingsPatch,
) -> Result<Settings, String> {
    let input = patch.apply(settings(connection)?);
    save_settings_values(
        connection,
        &input,
        patch.library_folder.is_some() || patch.route_downloads == Some(true),
        patch.device_name.as_deref(),
    )?;
    settings(connection)
}

pub(super) fn save_settings_values(
    connection: &Connection,
    input: &SettingsInput,
    validate_library: bool,
    device_name: Option<&str>,
) -> Result<(), String> {
    let device_name = device_name
        .map(crate::sync_meta::clean_device_name)
        .transpose()?;
    let (background_mode, launch_at_login) =
        crate::presence::normalize_presence(input.background_mode, input.launch_at_login);
    let library = input.library_folder.trim();
    if input.remote_auto_sync {
        crate::remote_sync::validate_root(input.remote_library_folder.trim())?;
    }
    if input.route_downloads && library.is_empty() {
        return Err("自動整理をオンにする前に、ライブラリのフォルダを選んでください".into());
    }
    if validate_library && !library.is_empty() {
        let folder = std::path::Path::new(library);
        if !folder.is_absolute()
            || folder
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(
                "ライブラリのフォルダは、絶対パスで親ディレクトリを含まないものにしてください"
                    .into(),
            );
        }
        match crate::library_files::probe_path(folder, crate::library_files::PROBE_TIMEOUT) {
            crate::library_files::Probe::Present => {}
            crate::library_files::Probe::Absent => {
                return Err("ライブラリのフォルダが見つかりません".into());
            }
            crate::library_files::Probe::Unreachable => {
                return Err("ライブラリのフォルダに届きません。接続を確認してからもう一度".into());
            }
        }
        if !std::fs::metadata(folder)
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
        {
            return Err("ライブラリのフォルダが見つかりません".into());
        }
        crate::scan::scan_block_reason(folder)?;
    }
    let tx = connection.unchecked_transaction().sql()?;
    let connection = &tx;
    set_setting(
        connection,
        "confidence_threshold",
        &input.confidence_threshold.clamp(0.0, 1.0).to_string(),
    )?;
    set_setting(
        connection,
        "browser_auto_register",
        if input.browser_auto_register {
            "1"
        } else {
            "0"
        },
    )?;
    set_setting(connection, "library_folder", input.library_folder.trim())?;
    forget_builtin_browser();
    set_setting(
        connection,
        "builtin_browser",
        if input.builtin_browser { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "browser_force_dark",
        if input.browser_force_dark { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "booth_link_target",
        &normalize_booth_link_target(&input.booth_link_target),
    )?;
    set_setting(
        connection,
        "remote_library_folder",
        input.remote_library_folder.trim(),
    )?;
    set_setting(
        connection,
        "remote_auto_sync",
        if input.remote_auto_sync { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "route_downloads",
        if input.route_downloads { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "background_mode",
        if background_mode { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "launch_at_login",
        if launch_at_login { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "classification_dismissed_version",
        &input.classification_dismissed_version.to_string(),
    )?;
    set_setting(
        connection,
        "ui_rail_width",
        &crate::model::clamp_ui_rail_width(input.ui_rail_width).to_string(),
    )?;
    set_setting(
        connection,
        "ui_rail_collapsed",
        if input.ui_rail_collapsed { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "ui_genre_width",
        &crate::model::clamp_ui_genre_width(input.ui_genre_width).to_string(),
    )?;
    set_setting(
        connection,
        "preview_pane",
        if input.preview_pane { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "preview_pane_width",
        &crate::model::clamp_preview_pane_width(input.preview_pane_width).to_string(),
    )?;
    set_setting(
        connection,
        "avatar_hero",
        &crate::model::normalize_avatar_hero(&input.avatar_hero),
    )?;
    set_setting(
        connection,
        "ui_genre_collapsed",
        if input.ui_genre_collapsed { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "right_panel_tabs",
        &crate::model::normalize_right_panel_tabs(&input.right_panel_tabs),
    )?;
    set_setting(
        connection,
        "check_update_on_startup",
        if input.check_update_on_startup {
            "1"
        } else {
            "0"
        },
    )?;
    set_setting(
        connection,
        "ui_update_dismissed_version",
        &input.ui_update_dismissed_version,
    )?;
    if let Some(name) = &device_name {
        set_setting(connection, crate::sync_meta::DEVICE_NAME_KEY, name)?;
    }
    crate::sync_meta::touch_config(&tx)?;
    tx.commit().sql()
}

pub(crate) fn browser_bookmarks(
    connection: &Connection,
) -> Result<Vec<crate::browser_tabs::Bookmark>, String> {
    match setting(connection, "browser_bookmarks")? {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| "ブックマークを読めません".into()),
        None => Ok(vec![
            crate::browser_tabs::Bookmark::new("BOOTH", "https://booth.pm/")?,
            crate::browser_tabs::Bookmark::new("購入ライブラリ", crate::browser::LIBRARY_URL)?,
        ]),
    }
}

pub(crate) fn save_browser_bookmark(
    connection: &Connection,
    original: Option<&str>,
    title: &str,
    url: &str,
) -> Result<(), String> {
    let mut bookmarks = browser_bookmarks(connection)?;
    crate::browser_tabs::save_bookmark(&mut bookmarks, original, title, url)?;
    set_setting(
        connection,
        "browser_bookmarks",
        &serde_json::to_string(&bookmarks)
            .map_err(|_| "ブックマークを保存できません".to_string())?,
    )
}

pub(crate) fn delete_browser_bookmark(connection: &Connection, url: &str) -> Result<(), String> {
    let mut bookmarks = browser_bookmarks(connection)?;
    bookmarks.retain(|bookmark| bookmark.url != url);
    set_setting(
        connection,
        "browser_bookmarks",
        &serde_json::to_string(&bookmarks)
            .map_err(|_| "ブックマークを保存できません".to_string())?,
    )
}

pub(crate) fn booth_page_interval_ms(connection: &Connection) -> Result<u64, String> {
    let stored = setting(connection, "booth_page_interval_ms")?
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(crate::library_walk::PAGE_DELAY_MS);
    Ok(crate::library_walk::normalize_page_delay_ms(stored))
}

pub(crate) fn set_booth_page_interval_ms(
    connection: &Connection,
    interval_ms: u64,
) -> Result<(), String> {
    if !crate::library_walk::allowed_page_delay_ms(interval_ms) {
        return Err("BOOTH のアクセス間隔は 250、500、1000 ミリ秒から選んでください".into());
    }
    set_setting(
        connection,
        "booth_page_interval_ms",
        &interval_ms.to_string(),
    )
}

/// 既定の Unity 本体のパス。未設定なら空文字。
pub(crate) fn unity_editor_path(connection: &Connection) -> Result<String, String> {
    Ok(setting(connection, "unity_editor_path")?.unwrap_or_default())
}

pub(crate) fn set_unity_editor_path(connection: &Connection, path: &str) -> Result<(), String> {
    set_setting(connection, "unity_editor_path", path)
}

pub(crate) fn extension_token(connection: &Connection) -> Result<String, String> {
    if let Some(existing) = setting(connection, "extension_token")?
        && crate::local_import::stored_token_ok(&existing)
    {
        return Ok(existing);
    }
    let token = crate::local_import::generate_token();
    set_setting(connection, "extension_token", &token)?;
    Ok(token)
}

pub(crate) fn extension_origin(connection: &Connection) -> Result<Option<String>, String> {
    let Some(value) = setting(connection, "extension_origin")? else {
        return Ok(None);
    };
    if crate::local_import::is_extension_origin(&value) {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

pub(crate) fn set_extension_origin(connection: &Connection, origin: &str) -> Result<(), String> {
    set_setting(connection, "extension_origin", origin)
}

pub(crate) fn regenerate_extension_token(connection: &Connection) -> Result<String, String> {
    let token = crate::local_import::generate_token();
    set_setting(connection, "extension_token", &token)?;
    connection
        .execute("DELETE FROM settings WHERE key = 'extension_origin'", [])
        .sql()?;
    Ok(token)
}

/// `builtin_browser` in memory: 0 not read yet, 1 off, 2 on. It is stored and cleared only while
/// the database connection is held (every write clears it before its transaction ends), so a read
/// can never keep a value older than the last write.
pub(super) static BUILTIN_BROWSER: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(0);

/// The built-in browser setting if it was read since the last write.
pub(crate) fn cached_builtin_browser() -> Option<bool> {
    match BUILTIN_BROWSER.load(std::sync::atomic::Ordering::SeqCst) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

/// Reads `builtin_browser` and keeps it for `cached_builtin_browser`. Call with the app's
/// connection locked.
pub(crate) fn builtin_browser(connection: &Connection) -> Result<bool, String> {
    let enabled = setting(connection, "builtin_browser")?.as_deref() == Some("1");
    BUILTIN_BROWSER.store(
        if enabled { 2 } else { 1 },
        std::sync::atomic::Ordering::SeqCst,
    );
    Ok(enabled)
}

/// A write of `builtin_browser` is coming. Call before the write, with the connection held.
pub(crate) fn forget_builtin_browser() {
    BUILTIN_BROWSER.store(0, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn setting(connection: &Connection, key: &str) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .sql()
}

pub(crate) fn set_setting(connection: &Connection, key: &str, value: &str) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .sql()?;
    Ok(())
}
