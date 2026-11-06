//! Real Tauri/GTK/WebKit coverage; run explicitly under a disposable X11 session.
use super::*;
use gtk::{gio, glib, prelude::*};
use serde_json::{Value, json};
use std::{process::Command, sync::Arc, time::Duration};
use webkit2gtk::WebViewExt;

type Check<T = ()> = Result<T, String>;
const TITLE: &str = "VRCLibMan native panel";

async fn javascript(view: &Webview, expression: &str) -> Check<Value> {
    let script = format!("JSON.stringify({expression})");
    let (send, receive) = tokio::sync::oneshot::channel();
    view.with_webview(move |native| {
        #[allow(deprecated)]
        native
            .inner()
            .run_javascript(&script, None::<&gio::Cancellable>, move |result| {
                let result = result.map_err(|error| error.to_string()).and_then(|value| {
                    let value = value.js_value().ok_or("JavaScript returned no value")?;
                    serde_json::from_str(&value.to_string()).map_err(|error| error.to_string())
                });
                let _ = send.send(result);
            });
    })
    .map_err(|error| error.to_string())?;
    glib::future_with_timeout(Duration::from_secs(5), receive)
        .await
        .map_err(|_| "JavaScript timed out")?
        .map_err(|_| "JavaScript callback closed")?
}

async fn wait_page(view: &Webview, id: &str) -> Check {
    for _ in 0..100 {
        if javascript(view, &format!("!!document.getElementById('{id}')")).await? == json!(true) {
            return Ok(());
        }
        glib::timeout_future(Duration::from_millis(40)).await;
    }
    Err(format!("Page did not load {id}"))
}

fn verify(value: bool, message: impl Into<String>) -> Check {
    if value { Ok(()) } else { Err(message.into()) }
}

async fn panel_rect(main: &Webview) -> Check<SurfaceArea> {
    let value = javascript(main, "(() => { const r=document.getElementById('panel').getBoundingClientRect(); const left=Math.ceil(r.left),top=Math.ceil(r.top); return {left,top,width:Math.floor(r.right)-left,height:Math.floor(r.bottom)-top}; })()").await?;
    let number = |key| {
        value[key]
            .as_f64()
            .ok_or_else(|| format!("Missing rectangle {key}: {value}"))
    };
    Ok(SurfaceArea {
        left: number("left")?,
        top: number("top")?,
        width: number("width")?,
        height: number("height")?,
    })
}

fn allocation(view: &Webview) -> Check<Value> {
    let (send, receive) = std::sync::mpsc::channel();
    view.with_webview(move |native| {
        let child = native.inner();
        let top = child.toplevel().unwrap();
        let (x,y) = child.translate_coordinates(&top, 0, 0).unwrap();
        let result = json!({"x":x,"y":y,"width":child.allocated_width(),"height":child.allocated_height(),"visible":child.is_visible(),"scale":child.scale_factor()});
        let _ = send.send(result);
    }).map_err(|error| error.to_string())?;
    receive.recv().map_err(|error| error.to_string())
}

async fn check_layout(app: &AppHandle, main: &Webview, page: &Webview) -> Check<SurfaceArea> {
    let area = panel_rect(main).await?;
    panel_area(app, Some(area), true)?;
    glib::timeout_future(Duration::from_millis(120)).await;
    javascript(page,"(()=>{window.nativePaintReady=false;requestAnimationFrame(()=>requestAnimationFrame(()=>window.nativePaintReady=true));return true;})()").await?;
    let mut painted = false;
    for _ in 0..100 {
        if javascript(page, "window.nativePaintReady").await? == json!(true) {
            painted = true;
            break;
        }
        glib::timeout_future(Duration::from_millis(20)).await;
    }
    verify(painted, "Page did not paint after native placement")?;
    let main_box = allocation(main)?;
    let page_box = allocation(page)?;
    let host = app
        .get_window(PANEL_WINDOW_LABEL)
        .ok_or("Missing host")?
        .default_vbox()
        .map_err(|error| error.to_string())?;
    let viewport = javascript(main, "({width:innerWidth,height:innerHeight})").await?;
    verify(
        main_box["width"] == json!(host.allocated_width())
            && main_box["height"] == json!(host.allocated_height()),
        format!(
            "Main webview shrank: {main_box}, host {}x{}",
            host.allocated_width(),
            host.allocated_height()
        ),
    )?;
    verify(
        main_box["width"] == viewport["width"] && main_box["height"] == viewport["height"],
        format!("Main viewport differs: {main_box} {viewport}"),
    )?;
    for (key, expected) in [
        ("x", area.left + main_box["x"].as_f64().unwrap()),
        ("y", area.top + main_box["y"].as_f64().unwrap()),
        ("width", area.width),
        ("height", area.height),
    ] {
        verify(
            (page_box[key].as_f64().unwrap() - expected).abs() <= 1.,
            format!("Native rectangle differs at {key}: {page_box}, expected {expected}"),
        )?;
    }
    let page_viewport = javascript(page, "({width:innerWidth,height:innerHeight})").await?;
    verify(
        (page_viewport["width"].as_f64().unwrap() - area.width).abs() <= 1.,
        format!("Page pixels have wrong width: {page_viewport}"),
    )?;
    Ok(area)
}

fn xdotool(args: &[String]) -> Check<String> {
    let executable = std::env::var("VRCLIBMAN_XDOTOOL").unwrap_or_else(|_| "xdotool".into());
    let output = Command::new(executable)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    verify(
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn click(main: &Webview, x: f64, y: f64) -> Check {
    let id = xdotool(&["search".into(), "--name".into(), format!("^{TITLE}$")])?;
    let main_box = allocation(main)?;
    let scale = main_box["scale"].as_f64().unwrap();
    let x = x + main_box["x"].as_f64().unwrap();
    let y = y + main_box["y"].as_f64().unwrap();
    xdotool(&[
        "mousemove".into(),
        "--window".into(),
        id,
        (x * scale).round().to_string(),
        (y * scale).round().to_string(),
        "click".into(),
        "1".into(),
    ])?;
    Ok(())
}

async fn exercise(app: AppHandle, address: String) -> Check {
    let main = app.get_webview(PANEL_WINDOW_LABEL).ok_or("Missing main")?;
    wait_page(&main, "panel").await?;
    ensure_panel_live(None, false);
    let window = app
        .get_window(PANEL_WINDOW_LABEL)
        .ok_or("Missing main window")?;
    let id = mount_tab(
        &app,
        PANEL_HOST_ID,
        &window,
        Url::parse(&format!("{address}/one")).unwrap(),
    )?;
    let page = view_for(&app, &id)?;
    wait_page(&page, "hit").await?;
    let area = check_layout(&app, &main, &page).await?;
    println!("native: main stays full-size and page matches panel rectangle");
    let identity = javascript(
        &page,
        "({nonce:window.nonce,cookie:document.cookie,session:sessionStorage.getItem('panel')})",
    )
    .await?;
    verify(
        identity["cookie"]
            .as_str()
            .unwrap()
            .contains("panel-session=kept"),
        "Browser cookie missing",
    )?;
    verify(
        javascript(&main, "document.cookie").await? == json!(""),
        "Browser profile leaked into main",
    )?;

    click(&main, 50., 40.)?;
    glib::timeout_future(Duration::from_millis(150)).await;
    verify(
        javascript(&main, "window.outsideClicks").await? == json!(1),
        "Overlay intercepted clicks outside the browser",
    )?;
    click(&main, area.left + 35., area.top + 35.)?;
    glib::timeout_future(Duration::from_millis(150)).await;
    verify(
        javascript(&page, "window.clicks").await? == json!(1),
        format!("Browser did not receive pointer input: {} native={} pointer={}", javascript(&page,"({clicks:window.clicks,pointer:window.pointer||null,scale:devicePixelRatio,width:innerWidth,height:innerHeight})").await?, allocation(&page)?, xdotool(&["getmouselocation".into()])?),
    )?;

    javascript(
        &main,
        "(document.getElementById('modal').hidden=false,true)",
    )
    .await?;
    panel_area(&app, Some(area), false)?;
    glib::timeout_future(Duration::from_millis(120)).await;
    verify(
        allocation(&page)?["visible"] == json!(false),
        "Covered page stayed visible",
    )?;
    click(&main, area.left + 35., area.top + 35.)?;
    glib::timeout_future(Duration::from_millis(150)).await;
    verify(
        javascript(&main, "window.modalClicks").await? == json!(1),
        "Hidden browser intercepted the dialog",
    )?;
    javascript(&main, "(document.getElementById('modal').hidden=true,true)").await?;
    check_layout(&app, &main, &page).await?;

    println!("native: pointer input and modal hiding passed");
    for (width, height) in [(1450., 900.), (1100., 700.), (1280., 800.)] {
        window
            .set_size(tauri::LogicalSize::new(width, height))
            .map_err(|error| error.to_string())?;
        glib::timeout_future(Duration::from_millis(200)).await;
        check_layout(&app, &main, &page).await?;
    }
    let second = mount_tab(
        &app,
        PANEL_HOST_ID,
        &window,
        Url::parse(&format!("{address}/two")).unwrap(),
    )?;
    let other = view_for(&app, &second)?;
    wait_page(&other, "hit").await?;
    check_layout(&app, &main, &other).await?;
    verify(
        allocation(&page)?["visible"] == json!(false),
        "Inactive tab stayed visible",
    )?;
    select(&app, &id)?;
    verify(
        allocation(&other)?["visible"] == json!(false),
        "Tab switch did not hide old page",
    )?;

    // Each move must retain the same native WebKit object and its live JS state.
    // Exercise both a whole-window dock and the single-tab dock path.
    for single_tab in [false, true, false] {
        undock(&app)?;
        select(&app, &id)?;
        glib::timeout_future(Duration::from_millis(250)).await;
        let native = allocation(&page)?;
        verify(
            native["x"] == json!(0),
            format!("Panel margin survived undock: {native}"),
        )?;
        verify(
            native["width"].as_i64().unwrap() > 800,
            format!("Detached page stayed panel-sized: {native}"),
        )?;
        if single_tab {
            dock_tab(&app, &id, None)?;
        } else {
            dock(&app, None, None)?;
        }
        select(&app, &id)?;
        check_layout(&app, &main, &page).await?;
        println!("native: dock/undock round trip, single tab={single_tab}");
        verify(javascript(&page, "({nonce:window.nonce,cookie:document.cookie,session:sessionStorage.getItem('panel')})").await? == identity, "Reparenting lost the live page or login data")?;
        if single_tab {
            dock(&app, None, None)?;
            select(&app, &id)?;
        }
    }

    // A different browser window must not keep receiving the panel's keys.
    detach_tab(&app, &second, 50., 50.)?;
    glib::timeout_future(Duration::from_millis(150)).await;
    let main_id = xdotool(&["search".into(), "--name".into(), format!("^{TITLE}$")])?;
    xdotool(&["windowraise".into(), main_id.clone()])?;
    xdotool(&["windowfocus".into(), "--sync".into(), main_id])?;
    let area = check_layout(&app, &main, &page).await?;
    click(&main, area.left + 35., area.top + 35.)?;
    glib::timeout_future(Duration::from_millis(150)).await;
    verify(
        window.is_focused().map_err(|error| error.to_string())?,
        "Main window did not receive focus",
    )?;
    verify(
        javascript(&page, "window.clicks").await? == json!(2),
        "Panel did not receive the click before shortcut",
    )?;
    xdotool(&["key".into(), "ctrl+equal".into()])?;
    glib::timeout_future(Duration::from_millis(200)).await;
    verify(
        (surface()
            .session
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .unwrap()
            .zoom_factor
            - 1.1)
            .abs()
            < 0.001,
        "Panel shortcut did not reach the focused page",
    )?;
    verify(
        surface()
            .session
            .tabs
            .iter()
            .find(|tab| tab.id == second)
            .unwrap()
            .zoom_factor
            == 1.,
        "Panel shortcut changed a different window",
    )?;
    zoom_tab(&app, &id, browser_tabs::ZoomChange::Reset)?;
    println!("native: panel shortcuts target the focused host");

    // Native navigation and history remain usable after the reparent operations.
    navigate_tab(&app, &id, &format!("{address}/two"))?;
    wait_page(&page, "hit").await?;
    for _ in 0..100 {
        if javascript(&page, "location.pathname").await? == json!("/two") {
            break;
        }
        glib::timeout_future(Duration::from_millis(30)).await;
    }
    history_tab(&app, &id, false)?;
    for _ in 0..100 {
        if javascript(&page, "location.pathname").await? == json!("/one") {
            break;
        }
        glib::timeout_future(Duration::from_millis(30)).await;
    }
    verify(
        javascript(&page, "location.pathname").await? == json!("/one"),
        "History was lost on moving the tab",
    )?;
    panel_area(&app, None, false)?;
    verify(
        allocation(&page)?["visible"] == json!(false),
        "Closing panel did not hide its page",
    )?;
    verify(
        app.get_webview(&id).is_some(),
        "Closing panel destroyed its page",
    )?;
    check_layout(&app, &main, &page).await?;
    close_tab(&app, &id)?;
    verify(
        app.get_window(PANEL_WINDOW_LABEL).is_some(),
        "Closing page destroyed main",
    )?;
    close(&app)?;
    println!(
        "native GTK panel: allocation, pointer input, dialog, resize, tabs, dock/undock, cookies, session, live JS and history passed"
    );
    Ok(())
}

#[test]
#[ignore = "requires Xvfb, xdotool, and isolated XDG data/config/cache directories"]
fn linux_browser_panel_native() {
    let test_root = std::env::var_os("VRCLIBMAN_NATIVE_TEST_DIR")
        .expect("set VRCLIBMAN_NATIVE_TEST_DIR to a disposable directory");
    let root = std::path::PathBuf::from(test_root);
    for name in ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"] {
        assert!(
            std::env::var_os(name)
                .map(std::path::PathBuf::from)
                .is_some_and(|path| path.is_absolute() && path.starts_with(&root)),
            "isolate {name} under VRCLIBMAN_NATIVE_TEST_DIR for the native test"
        );
    }
    std::fs::create_dir_all(&root).unwrap();
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let address = format!("http://{}", server.server_addr());
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let service = server.clone();
    let thread = std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            if let Ok(Some(request)) = service.recv_timeout(Duration::from_millis(50)) {
                let html = if request.url() == "/main" {
                    r#"<!doctype html><style>body{margin:0;background:#eee}#outside{position:absolute;left:20px;top:20px;width:100px;height:50px}#panel,#modal{position:fixed;right:0;top:120px;bottom:0;width:35vw}#modal{background:#f80}#modal button{position:absolute;left:20px;top:20px;width:100px;height:50px}</style><button id=outside onclick='outsideClicks++'>main</button><div id=panel></div><div id=modal hidden><button onclick='modalClicks++'>modal</button></div><script>window.outsideClicks=0;window.modalClicks=0</script>"#
                } else {
                    r#"<!doctype html><style>body{margin:0;background:#8d8}button{position:absolute;left:20px;top:20px;width:100px;height:50px}</style><button id=hit onclick='clicks++'>page</button><script>window.clicks=0;addEventListener('pointerdown',e=>window.pointer={x:e.clientX,y:e.clientY});window.nonce=Math.random();document.cookie='panel-session=kept; SameSite=Lax';sessionStorage.setItem('panel','kept')</script>"#
                };
                let header =
                    tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8")
                        .unwrap();
                let _ = request.respond(tiny_http::Response::from_string(html).with_header(header));
            }
        }
    });
    let result = Arc::new(Mutex::new(None));
    let outcome = result.clone();
    let app = tauri::Builder::default()
        .any_thread()
        .invoke_handler(tauri::generate_handler![
            crate::commands::browser_state,
            crate::commands::browser_action
        ])
        .on_window_event(handle_window_event)
        .setup(move |app| {
            let connection = crate::db::open(&root.join("library.sqlite"))?;
            crate::db::update_settings(
                &connection,
                &crate::model::SettingsPatch {
                    builtin_browser: Some(true),
                    ..Default::default()
                },
            )?;
            app.manage(crate::AppState {
                db: Mutex::new(connection),
                cancel_extract: AtomicBool::new(false),
                extract_running: Arc::new(AtomicBool::new(false)),
                thumbs_dir: root.join("thumbs"),
                previews_dir: root.join("previews"),
                hero_images_dir: root.join("hero_images"),
                receiver_error: Mutex::new(None),
                import_guard: Mutex::new(crate::local_import::ImportGuard::default()),
                background_mode: AtomicBool::new(false),
                staged_left: Mutex::new(Vec::new()),
            });
            tauri::WebviewWindowBuilder::new(
                app,
                PANEL_WINDOW_LABEL,
                tauri::WebviewUrl::External(format!("{address}/main").parse().unwrap()),
            )
            .title(TITLE)
            .inner_size(1280., 800.)
            .build()?;
            let handle = app.handle().clone();
            glib::MainContext::default().spawn_local(async move {
                let checked = glib::future_with_timeout(
                    Duration::from_secs(90),
                    exercise(handle.clone(), address),
                )
                .await
                .unwrap_or_else(|_| Err("Native browser check timed out".into()));
                let code = if checked.is_ok() { 0 } else { 1 };
                *outcome.lock().unwrap() = Some(checked);
                handle.exit(code);
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap();
    let code = app.run_return(|_, _| {});
    stopped.store(true, Ordering::Relaxed);
    thread.join().unwrap();
    assert_eq!(code, 0, "{:?}", result.lock().unwrap());
    result
        .lock()
        .unwrap()
        .take()
        .expect("native check did not finish")
        .unwrap();
}
