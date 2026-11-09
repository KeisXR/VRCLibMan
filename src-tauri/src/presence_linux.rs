//! AppIndicator can be constructed without a desktop host and still be invisible.
//! Check the session's StatusNotifier host before allowing a windowless process.
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use glib::variant::ToVariant;
use gtk::{gio, glib};
use tauri::{AppHandle, Manager};

use crate::presence;

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_INTERVAL: Duration = Duration::from_secs(2);
static HOST_AVAILABLE: AtomicBool = AtomicBool::new(false);
static TRAY_VISIBLE: AtomicBool = AtomicBool::new(false);

pub fn host_available() -> bool {
    HOST_AVAILABLE.load(Ordering::SeqCst)
}

pub fn tray_visible() -> bool {
    TRAY_VISIBLE.load(Ordering::SeqCst)
}

pub fn set_tray_visible(visible: bool) {
    TRAY_VISIBLE.store(visible, Ordering::SeqCst);
}

pub fn sync_login_item<R: tauri::Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), String> {
    let config = dirs::config_dir().ok_or("ログイン項目の保存先が見つかりません")?;
    let executable = app
        .env()
        .appimage
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .ok_or("ログイン時に起動するアプリが見つかりません")?;
    write_login_item(&config, &executable, enabled)
}

// auto-launch 0.6 ignores XDG_CONFIG_HOME on Linux. Write the standard entry in
// the user's configured directory, and refresh the outer AppImage path on start.
fn write_login_item(
    config: &std::path::Path,
    executable: &std::path::Path,
    enabled: bool,
) -> Result<(), String> {
    use std::io::Write;
    let directory = config.join("autostart");
    let path = directory.join("VRCLibMan.desktop");
    if !enabled {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("ログイン項目を削除できません".into()),
        };
    }
    let command = executable
        .to_str()
        .filter(|value| executable.is_absolute() && !value.chars().any(char::is_control))
        .ok_or("ログイン時に起動するアプリのパスが正しくありません")?;
    // GIO validates the executable before expanding field codes such as %%.
    // Use a constant exec wrapper so a literal '%' in the AppImage filename
    // works too. The filename is an argument, never part of the shell program.
    let command = [
        "/bin/sh",
        "-c",
        "exec \"$@\"",
        "vrclibman",
        command,
        presence::AUTOSTART_ARG,
    ]
    .map(desktop_argument)
    .join(" ");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=VRCLibMan\nExec={command}\nTerminal=false\nStartupNotify=false\n"
    );
    std::fs::create_dir_all(&directory).map_err(|_| "ログイン項目の保存先を作れません")?;
    let mut file =
        tempfile::NamedTempFile::new_in(&directory).map_err(|_| "ログイン項目を作れません")?;
    file.write_all(entry.as_bytes())
        .map_err(|_| "ログイン項目を書き込めません")?;
    file.persist(path)
        .map_err(|_| "ログイン項目を保存できません")?;
    Ok(())
}

// Exec quoting is decoded after desktop-entry string escaping. Literal '%'
// must also be escaped so filename characters cannot become field codes.
fn desktop_argument(value: &str) -> String {
    let mut quoted = String::new();
    for ch in value.chars() {
        if ch == '%' {
            quoted.push_str("%%");
        } else {
            if matches!(ch, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(ch);
        }
    }
    format!("\"{}\"", quoted.replace('\\', "\\\\"))
}

// Resolve the unique owner first. A new owner means the shell restarted: its old
// registration must not be treated as a usable recovery path.
async fn probe(connection: &gio::DBusConnection) -> Option<String> {
    let reply = connection
        .call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetNameOwner",
            Some(&(WATCHER,).to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            800,
        )
        .await
        .ok()?;
    let (owner,) = reply.get::<(String,)>()?;
    let reply = connection
        .call_future(
            Some(&owner),
            WATCHER_PATH,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&(WATCHER, "IsStatusNotifierHostRegistered").to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            800,
        )
        .await
        .ok()?;
    let (value,) = reply.get::<(glib::Variant,)>()?;
    value.get::<bool>()?.then_some(owner)
}

async fn desktop_host() -> Option<String> {
    let connection = gio::bus_get_future(gio::BusType::Session).await.ok()?;
    connection.set_exit_on_close(false);
    probe(&connection).await
}

pub fn start(app: AppHandle, login_launch: bool) {
    // Both GIO futures and the native tray operate on GTK's main context. The
    // bounded, sequential probes never block that thread or accumulate workers.
    glib::MainContext::default().spawn_local(async move {
        let mut previous_owner = None;
        let mut first = true;
        let mut reported_error = false;
        loop {
            let owner = glib::future_with_timeout(PROBE_TIMEOUT, desktop_host())
                .await
                .unwrap_or(None);
            let was_ready = presence::background_mode_enabled(&app);
            HOST_AVAILABLE.store(owner.is_some(), Ordering::SeqCst);
            if previous_owner != owner {
                presence::remove_tray(&app);
                previous_owner = owner;
                reported_error = false;
            }
            if !presence::background_requested(&app) {
                reported_error = false;
            }
            // A failed native construction can retain a libappindicator object.
            // Retry on host change or an explicit settings operation, not every poll.
            if presence::background_requested(&app) && host_available() && !reported_error {
                if let Err(error) = presence::ensure_tray(&app) {
                    if !reported_error {
                        eprintln!("{error}");
                    }
                    reported_error = true;
                } else {
                    reported_error = false;
                }
            }
            let ready = presence::background_mode_enabled(&app);
            // Preserve the saved preference. A missing/late host or a failed
            // tray only changes this session's ability to close into the tray.
            if (first && login_launch && presence::should_open_window_on_start(ready, true))
                || (was_ready && !ready)
            {
                if let Err(error) = presence::show_main_window(&app) {
                    eprintln!("{error}");
                }
            }
            first = false;
            glib::timeout_future(PROBE_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
        sync::Arc,
    };

    #[test]
    fn xdg_autostart_launches_an_appimage_path_with_reserved_characters() {
        use gio::prelude::*;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir
            .path()
            .join("VRCLibMan % \" $ ` \\ with = spaces.AppImage");
        let marker = dir.path().join("launched.txt");
        std::fs::write(
            &executable,
            format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        write_login_item(dir.path(), &executable, true).unwrap();
        let path = dir.path().join("autostart/VRCLibMan.desktop");
        let entry = gio::DesktopAppInfo::from_filename(&path).unwrap();
        entry.launch(&[], None::<&gio::AppLaunchContext>).unwrap();
        for _ in 0..100 {
            if std::fs::read_to_string(&marker).ok().as_deref() == Some(presence::AUTOSTART_ARG) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            std::fs::read_to_string(marker).unwrap(),
            presence::AUTOSTART_ARG
        );
        let moved = dir.path().join("moved.AppImage");
        write_login_item(dir.path(), &moved, true).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains(&moved.to_string_lossy().to_string())
        );
        std::fs::write(dir.path().join("autostart/unrelated.desktop"), "leave me").unwrap();
        write_login_item(dir.path(), &moved, false).unwrap();
        assert!(!path.exists());
        assert!(dir.path().join("autostart/unrelated.desktop").exists());
        write_login_item(dir.path(), &moved, false).unwrap();
        assert!(write_login_item(dir.path(), std::path::Path::new("relative"), true).is_err());
        assert!(
            write_login_item(dir.path(), std::path::Path::new("/invalid\npath"), true).is_err()
        );
        assert!(!path.exists());
    }

    struct SessionBus(Child);
    impl Drop for SessionBus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    // A real, private D-Bus exercises the message signatures and property reads,
    // including a watcher with no display host and shell replacement. No GTK
    // display, real session bus, credentials or user configuration is used.
    #[test]
    fn status_notifier_probe_requires_a_host_and_tracks_shell_replacement() {
        let mut bus = SessionBus(
            Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address=1"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let mut address = String::new();
        BufReader::new(bus.0.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let context = glib::MainContext::new();
        context.with_thread_default(|| context.block_on(async {
            let connect = || gio::DBusConnection::for_address_future(
                address.trim(), gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                    | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION, None);
            let client = connect().await.unwrap();
            client.set_exit_on_close(false);
            assert_eq!(probe(&client).await, None);
            let info = gio::DBusNodeInfo::for_xml(&format!(
                "<node><interface name='{WATCHER}'><property name='IsStatusNotifierHostRegistered' type='b' access='read'/></interface></node>"
            )).unwrap().lookup_interface(WATCHER).unwrap();
            let available = Arc::new(AtomicBool::new(false));
            let watcher = connect().await.unwrap();
            watcher.set_exit_on_close(false);
            let flag = available.clone();
            let _object = watcher.register_object(WATCHER_PATH, &info,
                |_, _, _, _, _, _, invocation| { invocation.return_value(None); },
                move |_, _, _, _, _| flag.load(Ordering::SeqCst).to_variant(),
                |_, _, _, _, _, _| false).unwrap();
            watcher.call_future(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus",
                "org.freedesktop.DBus", "RequestName", Some(&(WATCHER, 0u32).to_variant()),
                None, gio::DBusCallFlags::NONE, 800).await.unwrap();
            assert_eq!(probe(&client).await, None);
            available.store(true, Ordering::SeqCst);
            let first_owner = probe(&client).await.unwrap();
            available.store(false, Ordering::SeqCst);
            assert_eq!(probe(&client).await, None);
            watcher.close_future().await.unwrap();
            assert_eq!(probe(&client).await, None);
            let replacement = connect().await.unwrap();
            replacement.set_exit_on_close(false);
            let _object = replacement.register_object(WATCHER_PATH, &info,
                |_, _, _, _, _, _, invocation| { invocation.return_value(None); },
                |_, _, _, _, _| true.to_variant(),
                |_, _, _, _, _, _| false).unwrap();
            replacement.call_future(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus",
                "org.freedesktop.DBus", "RequestName", Some(&(WATCHER, 0u32).to_variant()),
                None, gio::DBusCallFlags::NONE, 800).await.unwrap();
            assert_ne!(probe(&client).await.unwrap(), first_owner);
            replacement.close_future().await.unwrap();
            client.close_future().await.unwrap();
        })).unwrap();
    }
}
