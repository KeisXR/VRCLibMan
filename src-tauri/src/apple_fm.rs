use crate::model::Extraction;
use crate::ollama::parse_extraction;
use serde::Serialize;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleStatus {
    pub availability: String,
    pub variant: String,
    pub context_size: u32,
    pub message: String,
}

#[derive(Debug)]
pub enum AppleError {
    Guardrail,
    Message(String),
}

pub struct Resident {
    child: Child,
    stdin: std::process::ChildStdin,
    lines: mpsc::Receiver<std::io::Result<String>>,
}

pub fn helper_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("VRCLIBMAN_AFM_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    helper_in_dir(dir)
}

pub(crate) fn helper_in_dir(dir: &Path) -> Option<PathBuf> {
    // Tauri copies the sidecar into the app as vrclibman-afm, without the target triple.
    for name in ["vrclibman-afm", sidecar_name().as_str()] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn sidecar_name() -> String {
    let os = match std::env::consts::OS {
        "macos" => "apple-darwin",
        "windows" => "pc-windows-msvc",
        "linux" => "unknown-linux-gnu",
        other => other,
    };
    format!("vrclibman-afm-{}-{os}", std::env::consts::ARCH)
}

pub fn os_supports_apple_fm() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_version_at_least(27, 2)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(any(target_os = "macos", test))]
pub fn version_at_least(text: &str, major: u32, minor: u32) -> bool {
    let mut parts = text.trim().split('.');
    let got_major = parts.next().unwrap_or("0").parse::<u32>().unwrap_or(0);
    let got_minor = parts.next().unwrap_or("0").parse::<u32>().unwrap_or(0);
    got_major > major || (got_major == major && got_minor >= minor)
}

#[cfg(target_os = "macos")]
fn macos_version_at_least(major: u32, minor: u32) -> bool {
    let Ok(output) = Command::new("/usr/bin/sw_vers").arg("-productVersion").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    version_at_least(text.trim(), major, minor)
}

fn unsupported_status() -> AppleStatus {
    AppleStatus {
        availability: "unsupported".into(),
        variant: String::new(),
        context_size: 0,
        message: "Apple のオンデバイスモデルは macOS 27.2 以上で使えます".into(),
    }
}

pub fn status(path: Option<&Path>) -> AppleStatus {
    if !os_supports_apple_fm() {
        return unsupported_status();
    }
    let owned = helper_path();
    let Some(path) = path.or_else(|| owned.as_deref()) else {
        return AppleStatus {
            availability: "unsupported".into(),
            variant: String::new(),
            context_size: 0,
            message: "Apple のオンデバイスモデルは、この PC では使えません".into(),
        };
    };
    match availability_of(&path, Duration::from_secs(8)) {
        Ok(status) => status,
        Err(message) => AppleStatus {
            availability: "unavailable".into(),
            variant: String::new(),
            context_size: 0,
            message,
        },
    }
}

pub fn can_select(path: Option<&Path>) -> bool {
    status(path).availability == "available"
}

pub fn extract_warm(
    instructions: &str,
    title: &str,
    description: &str,
    timeout: Duration,
) -> Result<Extraction, AppleError> {
    if !os_supports_apple_fm() {
        return Err(AppleError::Message(
            "Apple のオンデバイスモデルは macOS 27.2 以上で使えます".into(),
        ));
    }
    let path = helper_path().ok_or_else(|| {
        AppleError::Message("Apple のオンデバイスモデルは、この PC では使えません".into())
    })?;
    let mut slot = resident_slot()
        .lock()
        .map_err(|_| AppleError::Message("Apple のオンデバイスモデルを実行できません".into()))?;
    if slot.is_none() {
        *slot = Some(Resident::spawn(&path).map_err(AppleError::Message)?);
    }
    let result = slot
        .as_mut()
        .expect("resident")
        .extract(instructions, title, description, timeout);
    if let Err(AppleError::Message(ref message)) = result {
        if message.contains("タイムアウト") || message.contains("終了") || message.contains("読めません") {
            *slot = None;
        }
    }
    result
}

fn resident_slot() -> &'static std::sync::Mutex<Option<Resident>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Option<Resident>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(None))
}

fn availability_of(path: &Path, timeout: Duration) -> Result<AppleStatus, String> {
    let mut child = Command::new(path)
        .arg("--availability")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Apple のオンデバイスモデルを起動できません".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Apple のオンデバイスモデルの応答を読めません".to_string())?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map(|_| line);
        let _ = tx.send(result);
    });
    let line = match rx.recv_timeout(timeout) {
        Ok(Ok(line)) => line,
        Ok(Err(_)) => {
            let _ = child.kill();
            return Err("Apple のオンデバイスモデルの応答を読めません".into());
        }
        Err(_) => {
            let _ = child.kill();
            return Err("Apple のオンデバイスモデルの確認がタイムアウトしました".into());
        }
    };
    let _ = child.wait();
    parse_availability(&line)
}

pub fn parse_availability(line: &str) -> Result<AppleStatus, String> {
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|_| "Apple のオンデバイスモデルの応答を解釈できません".to_string())?;
    let availability = value
        .get("availability")
        .and_then(Value::as_str)
        .unwrap_or("unavailable")
        .to_string();
    let variant = value
        .get("variant")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let context_size = value
        .get("contextSize")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let message = if availability == "available" {
        "Apple のオンデバイスモデルを使えます".into()
    } else if reason.is_empty() {
        "Apple のオンデバイスモデルは今は使えません".into()
    } else {
        format!("Apple のオンデバイスモデルは今は使えません（{reason}）")
    };
    Ok(AppleStatus {
        availability,
        variant,
        context_size,
        message,
    })
}

impl Resident {
    pub fn spawn(path: &Path) -> Result<Self, String> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Apple のオンデバイスモデルを起動できません".to_string())?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Apple のオンデバイスモデルへ送れません".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Apple のオンデバイスモデルの応答を読めません".to_string())?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            lines: rx,
        })
    }

    pub fn extract(
        &mut self,
        instructions: &str,
        title: &str,
        description: &str,
        timeout: Duration,
    ) -> Result<Extraction, AppleError> {
        let request = json!({
            "title": title,
            "description": description,
            "instructions": instructions,
        });
        let line = serde_json::to_string(&request)
            .map_err(|_| AppleError::Message("依頼を書けません".into()))?;
        writeln!(self.stdin, "{line}")
            .and_then(|_| self.stdin.flush())
            .map_err(|_| AppleError::Message("Apple のオンデバイスモデルへ送れません".into()))?;
        let response = match self.lines.recv_timeout(timeout) {
            Ok(Ok(line)) => line,
            Ok(Err(_)) => {
                return Err(AppleError::Message(
                    "Apple のオンデバイスモデルの応答を読めません".into(),
                ));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                return Err(AppleError::Message(
                    "Apple のオンデバイスモデルの応答がタイムアウトしました".into(),
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AppleError::Message(
                    "Apple のオンデバイスモデルが終了しました".into(),
                ));
            }
        };
        parse_generation(&response)
    }
}

pub fn parse_generation(line: &str) -> Result<Extraction, AppleError> {
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|_| AppleError::Message("Apple のオンデバイスモデルの応答を解釈できません".into()))?;
    let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if !ok {
        let code = value.get("error").and_then(Value::as_str).unwrap_or("");
        if code == "guardrailViolation" {
            return Err(AppleError::Guardrail);
        }
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Apple のオンデバイスモデルが応答できません");
        return Err(AppleError::Message(message.to_string()));
    }
    let json = value.get("json").cloned().unwrap_or(Value::Null);
    let text = if json.is_string() {
        json.as_str().unwrap_or("").to_string()
    } else {
        json.to_string()
    };
    parse_extraction(&text).map_err(|err| {
        AppleError::Message(format!("Apple のオンデバイスモデルの応答を解釈できません: {err}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn finds_the_bundled_helper_without_a_target_triple() {
        let dir = std::env::temp_dir().join(format!(
            "vrclibman-afm-names-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bare = dir.join("vrclibman-afm");
        std::fs::write(&bare, b"helper").unwrap();
        assert_eq!(helper_in_dir(&dir).unwrap(), bare);
        std::fs::remove_file(&bare).unwrap();
        let triple = dir.join(sidecar_name());
        std::fs::write(&triple, b"helper").unwrap();
        assert_eq!(helper_in_dir(&dir).unwrap(), triple);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apple_fm_requires_macos_27_2() {
        assert!(version_at_least("27.2", 27, 2));
        assert!(version_at_least("27.2.1", 27, 2));
        assert!(version_at_least("28.0", 27, 2));
        assert!(!version_at_least("27.1.9", 27, 2));
        assert!(!version_at_least("26.0", 27, 2));
        assert!(!version_at_least("14.6.1", 27, 2));
        #[cfg(not(target_os = "macos"))]
        {
            let report = status(None);
            assert_eq!(report.availability, "unsupported");
            assert!(report.message.contains("27.2"));
            assert!(extract_warm("指示", "題", "説明", Duration::from_secs(1)).is_err());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn real_helper_answers_availability_when_the_os_can_run_it() {
        if !os_supports_apple_fm() {
            eprintln!("macOS 27.2 未満なので飛ばします");
            return;
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries").join(format!(
            "vrclibman-afm-{}-apple-darwin",
            std::env::consts::ARCH
        ));
        if !path.is_file() {
            eprintln!("補助プログラムが無いので飛ばします");
            return;
        }
        match availability_of(&path, Duration::from_secs(20)) {
            Ok(report) if report.availability == "available" => {
                assert!(report.context_size > 0, "{}", report.message);
            }
            Ok(report) => eprintln!("モデルが使えないので飛ばします: {}", report.message),
            Err(err) => panic!("補助プログラムが --availability に応答しません: {err}"),
        }
    }

    #[test]
    fn generation_json_strips_tool_names_and_keeps_versions() {
        let parsed = parse_generation(
            r#"{"ok":true,"json":{"category":"outfit","compatible_avatars":["クルミ ver1.2","lilToon"],"is_generic":false,"confidence":0.9}}"#,
        )
        .unwrap();
        assert_eq!(
            parsed.compatible_avatars,
            vec!["クルミ ver1.2".to_string()]
        );
        let blocked = parse_generation(r#"{"ok":false,"error":"guardrailViolation"}"#);
        assert!(matches!(blocked, Err(AppleError::Guardrail)));
    }

    #[cfg(unix)]
    #[test]
    fn availability_and_generation_use_the_helper_protocol() {
        let path = write_fake_helper();
        let status = availability_of(&path, Duration::from_secs(2)).unwrap();
        assert_eq!(status.availability, "available");
        assert_eq!(status.variant, "default");
        assert_eq!(status.context_size, 8192);
        let mut resident = Resident::spawn(&path).unwrap();
        let extracted = resident
            .extract("指示", "タイトル", "説明", Duration::from_secs(2))
            .unwrap();
        assert_eq!(extracted.category, crate::model::Category::Outfit);
        assert_eq!(extracted.compatible_avatars, vec!["クルミ ver1.2".to_string()]);
        let blocked = resident.extract("指示", "タイトル", "GUARD", Duration::from_secs(2));
        assert!(matches!(blocked, Err(AppleError::Guardrail)));
    }

    #[cfg(unix)]
    #[test]
    fn a_slow_helper_times_out() {
        let path = write_slow_helper();
        let err = availability_of(&path, Duration::from_millis(200)).unwrap_err();
        assert!(err.contains("タイムアウト"));
    }

    #[test]
    fn unavailable_stub_reports_that_apple_fm_is_not_usable() {
        let status = std::process::Command::new("cc").arg("--version").status();
        if !status.map(|code| code.success()).unwrap_or(false) {
            eprintln!("cc が無いのでスタブの実行は飛ばします");
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "vrclibman-afm-stub-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join(if cfg!(windows) { "stub.exe" } else { "stub" });
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("afm/unavailable-stub.c");
        let compiled = std::process::Command::new("cc")
            .arg("-O2")
            .arg("-o")
            .arg(&bin)
            .arg(&source)
            .status()
            .unwrap();
        assert!(compiled.success());
        let output = std::process::Command::new(&bin)
            .arg("--availability")
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("\"availability\":\"unavailable\""), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    fn write_fake_helper() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "vrclibman-afm-fake-{}-{}.sh",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r#"#!/bin/sh
if [ "$1" = "--availability" ]; then
  printf '%s\n' '{"availability":"available","variant":"default","contextSize":8192}'
  exit 0
fi
while IFS= read -r line; do
  case "$line" in
    *GUARD*) printf '%s\n' '{"ok":false,"error":"guardrailViolation","message":"blocked"}' ;;
    *) printf '%s\n' '{"ok":true,"json":{"category":"outfit","compatible_avatars":["クルミ ver1.2","lilToon"],"is_generic":false,"confidence":0.9}}' ;;
  esac
done
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[cfg(unix)]
    fn write_slow_helper() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "vrclibman-afm-slow-{}-{}.sh",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }
}
