use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Unavailable,
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Ready,
    Installing,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub revision: u64,
    pub phase: Phase,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
    pub message: Option<String>,
}

struct Inner {
    status: Status,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}

pub struct UpdateState(Mutex<Inner>);

impl UpdateState {
    pub fn new(configured: bool, supported: bool) -> Self {
        Self(Mutex::new(Inner {
            status: Status {
                revision: 0,
                phase: if configured && supported {
                    Phase::Idle
                } else {
                    Phase::Unavailable
                },
                version: None,
                notes: None,
                downloaded: 0,
                total: None,
                error: None,
                message: if !supported {
                    Some(
                        "アプリ内更新は Windows x64 と Apple Silicon Mac の配布版で利用できます。"
                            .into(),
                    )
                } else if !configured {
                    Some("このビルドはアプリ内更新に対応していません。".into())
                } else {
                    None
                },
            },
            update: None,
            bytes: None,
        }))
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>, String> {
        self.0.lock().map_err(|_| "更新状態を確認できません".into())
    }

    pub fn status(&self) -> Result<Status, String> {
        Ok(self.lock()?.status.clone())
    }
}

impl Inner {
    fn phase(&mut self, phase: Phase) {
        self.status.phase = phase;
        self.status.error = None;
        self.status.revision += 1;
    }

    fn ensure_idle(&self) -> Result<(), String> {
        match self.status.phase {
            Phase::Unavailable => Err(self.status.message.clone().unwrap_or_default()),
            Phase::Checking | Phase::Downloading | Phase::Installing => {
                Err("更新処理が進行中です".into())
            }
            _ => Ok(()),
        }
    }

    fn failed(&mut self, phase: Phase, error: String) -> Status {
        self.phase(phase);
        self.status.error = Some(error);
        self.status.clone()
    }
}

pub fn initialize(app: &AppHandle) {
    let configured = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(|key| key.as_str())
        .is_some_and(|key| !key.trim().is_empty());
    let supported = cfg!(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64")
    ));
    app.manage(UpdateState::new(configured, supported));
}

pub async fn check(app: &AppHandle, state: &UpdateState) -> Result<Status, String> {
    {
        let mut inner = state.lock()?;
        inner.ensure_idle()?;
        // Keep an already verified download until it is applied or the app exits.
        if inner.status.phase == Phase::Ready {
            return Ok(inner.status.clone());
        }
        inner.update = None;
        inner.bytes = None;
        inner.status.version = None;
        inner.status.notes = None;
        inner.status.downloaded = 0;
        inner.status.total = None;
        inner.phase(Phase::Checking);
    }
    let result = async {
        app.updater_builder()
            .timeout(Duration::from_secs(30))
            .build()?
            .check()
            .await
    }
    .await;
    let mut inner = state.lock()?;
    match result {
        Ok(Some(update)) => {
            // The plugin checks the signature, but does not enforce HTTPS for the artifact.
            if update.download_url.scheme() != "https" {
                return Ok(inner.failed(
                    Phase::Idle,
                    "更新ファイルの URL が HTTPS ではありません".into(),
                ));
            }
            inner.status.version = Some(update.version.clone());
            inner.status.notes = update.body.clone();
            inner.update = Some(update);
            inner.phase(Phase::Available);
        }
        Ok(None) => inner.phase(Phase::Current),
        Err(error) => {
            return Ok(inner.failed(
                Phase::Idle,
                update_error("更新を確認できませんでした", &error),
            ));
        }
    }
    Ok(inner.status.clone())
}

pub async fn download(state: &UpdateState) -> Result<Status, String> {
    let mut update = {
        let mut inner = state.lock()?;
        inner.ensure_idle()?;
        if inner.status.phase == Phase::Ready {
            return Ok(inner.status.clone());
        }
        let update = inner.update.clone().ok_or("先に更新を確認してください")?;
        inner.status.downloaded = 0;
        inner.status.total = None;
        inner.phase(Phase::Downloading);
        update
    };
    // A slow download should have longer than the short metadata request timeout.
    update.timeout = Some(Duration::from_secs(15 * 60));
    let result = update
        .download(
            |length, total| {
                if let Ok(mut inner) = state.lock() {
                    inner.status.downloaded = inner.status.downloaded.saturating_add(length as u64);
                    inner.status.total = total;
                    inner.status.revision += 1;
                }
            },
            || {},
        )
        .await;
    let mut inner = state.lock()?;
    match result {
        Ok(bytes) => {
            // Update::download returns only after signature verification succeeds.
            inner.bytes = Some(bytes);
            inner.phase(Phase::Ready);
        }
        Err(error) => {
            inner.bytes = None;
            return Ok(inner.failed(
                Phase::Available,
                update_error("更新ファイルを取得できませんでした", &error),
            ));
        }
    }
    Ok(inner.status.clone())
}

pub async fn install(app: &AppHandle, state: &UpdateState) -> Result<Status, String> {
    let (update, bytes) = {
        let mut inner = state.lock()?;
        inner.ensure_idle()?;
        if inner.status.phase != Phase::Ready {
            return Err("先に更新をダウンロードしてください".into());
        }
        let app_state = app.state::<crate::AppState>();
        if app_state
            .extract_running
            .load(std::sync::atomic::Ordering::SeqCst)
            || crate::browser::library_walk_running()
            || crate::browser::download_job_armed()
        {
            return Err("分類・インポート・ダウンロードが終わってから更新してください".into());
        }
        let update = inner.update.clone().ok_or("更新情報がありません")?;
        let bytes = inner.bytes.take().ok_or("更新ファイルがありません")?;
        inner.phase(Phase::Installing);
        (update, bytes)
    };
    // Installing can block on disk I/O and platform UI. Never hold the state/DB lock.
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = update.install(&bytes);
        (result, bytes)
    })
    .await;
    match result {
        Ok((Ok(()), _)) => {
            // Windows exits inside install; macOS needs an explicit restart.
            app.restart();
        }
        Ok((Err(error), bytes)) => {
            let mut inner = state.lock()?;
            inner.bytes = Some(bytes);
            Ok(inner.failed(
                Phase::Ready,
                update_error("更新を適用できませんでした", &error),
            ))
        }
        Err(_) => {
            let mut inner = state.lock()?;
            Ok(inner.failed(
                Phase::Available,
                "更新処理が中断されました。もう一度ダウンロードしてください".into(),
            ))
        }
    }
}

fn update_error(context: &str, error: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    let detail = match error {
        Error::ReleaseNotFound => {
            "更新情報が見つかりません。GitHub Releases で最新バージョンを確認してください。"
        }
        Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            "この OS・CPU 向けの更新ファイルがありません。"
        }
        Error::Minisign(_)
        | Error::Base64(_)
        | Error::SignatureUtf8(_)
        | Error::SignedVersionMismatch { .. }
        | Error::MissingSignedVersion => {
            "更新ファイルの署名を確認できません。適用せず停止しました。"
        }
        Error::Reqwest(_) | Error::Network(_) => {
            "ネットワーク接続を確認して、もう一度お試しください。"
        }
        _ => {
            "アプリのインストール先と空き容量を確認してください。解決しない場合は GitHub Releases から手動で更新してください。"
        }
    };
    format!("{context}。{detail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_or_unsupported_platform_never_enables_updates() {
        for (configured, supported) in [(false, true), (true, false), (false, false)] {
            let state = UpdateState::new(configured, supported);
            assert_eq!(state.status().unwrap().phase, Phase::Unavailable);
            assert!(state.lock().unwrap().ensure_idle().is_err());
        }
        assert_eq!(
            UpdateState::new(true, true).status().unwrap().phase,
            Phase::Idle
        );
    }

    #[test]
    fn active_operations_exclude_other_operations_and_failures_allow_retry() {
        let state = UpdateState::new(true, true);
        let mut inner = state.lock().unwrap();
        for phase in [Phase::Checking, Phase::Downloading, Phase::Installing] {
            inner.phase(phase);
            assert!(inner.ensure_idle().is_err());
        }
        let failed = inner.failed(Phase::Available, "接続できません".into());
        assert!(failed.error.is_some());
        assert!(inner.ensure_idle().is_ok());
        inner.phase(Phase::Downloading);
        assert!(inner.status.error.is_none());
        assert!(inner.status.revision > failed.revision);
    }

    fn test_app() -> tauri::App<tauri::test::MockRuntime> {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "pubkey": include_str!("../test-fixtures/updater/public-key.txt").trim(),
                "endpoints": ["https://example.invalid/latest.json"],
                "requireSignedVersion": true,
            }),
        );
        tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap()
    }

    fn restored_update(
        app: &tauri::App<tauri::test::MockRuntime>,
        url: &str,
        version: &str,
    ) -> Option<Update> {
        let mut builder = app.updater_builder().no_proxy();
        // On macOS the test executable is not inside an application bundle.
        if cfg!(target_os = "macos") {
            builder =
                builder.executable_path("/Applications/VRCLibMan.app/Contents/MacOS/VRCLibMan");
        }
        builder
            .build()
            .unwrap()
            .restore_update(serde_json::json!({
                "version": version,
                "url": url,
                "signature": include_str!("../test-fixtures/updater/payload.txt.sig").trim(),
            }))
            .unwrap()
    }

    #[test]
    fn authentic_signature_rejects_tampering_wrong_versions_and_downgrades() {
        let app = test_app();
        let update = restored_update(&app, "https://example.invalid/update", "99.0.0").unwrap();
        let bytes = include_bytes!("../test-fixtures/updater/payload.txt");
        update.verify(bytes).unwrap();
        assert!(update.verify(b"tampered").is_err());
        // install must also verify before doing any platform-specific work.
        assert!(update.install(b"tampered").is_err());
        let substituted =
            restored_update(&app, "https://example.invalid/update", "100.0.0").unwrap();
        assert!(matches!(
            substituted.verify(bytes),
            Err(tauri_plugin_updater::Error::SignedVersionMismatch { .. })
        ));
        assert!(restored_update(&app, "https://example.invalid/update", "0.0.0").is_none());
    }

    #[test]
    fn failed_download_keeps_update_retryable_and_only_verified_bytes_become_ready() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/update", server.server_addr());
        let sender = std::thread::spawn(move || {
            for bytes in [
                b"tampered".as_slice(),
                include_bytes!("../test-fixtures/updater/payload.txt").as_slice(),
            ] {
                server
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .expect("update request")
                    .respond(tiny_http::Response::from_data(bytes))
                    .unwrap();
            }
        });
        let app = test_app();
        let state = UpdateState::new(true, true);
        {
            let mut inner = state.lock().unwrap();
            inner.update = restored_update(&app, &url, "99.0.0");
            inner.phase(Phase::Available);
        }
        tauri::async_runtime::block_on(async {
            let failed = download(&state).await.unwrap();
            assert_eq!(failed.phase, Phase::Available);
            assert!(failed.error.unwrap().contains("署名"));
            assert!(state.lock().unwrap().bytes.is_none());
            let ready = download(&state).await.unwrap();
            assert_eq!(ready.phase, Phase::Ready);
            assert!(ready.error.is_none());
            assert_eq!(
                state.lock().unwrap().bytes.as_deref().unwrap(),
                include_bytes!("../test-fixtures/updater/payload.txt")
            );
            // Repeated download does not re-fetch or discard the verified package.
            assert_eq!(download(&state).await.unwrap().phase, Phase::Ready);
        });
        sender.join().unwrap();
    }
}
