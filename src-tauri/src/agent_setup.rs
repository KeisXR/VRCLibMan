//! AI エージェントごとの MCP 設定と SKILL の配布。
//!
//! 書くのは、下の表に載せたエージェントの設定ファイルの `vrclibman` の項目と、
//! 印（`.vrclibman-skill`）のある skill フォルダだけ。ほかの項目とサーバーは並びも含めて残す。
//! 書く前に元のファイルを `<ファイル>.vrclibman-backup` に写し、同じフォルダの一時ファイルから置き換える。
//! コメント付きの JSON（JSONC）は書き戻せないので書かない。ネットワークには出ない。
//! 表と出典は docs/agent-setup.md。
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) const SERVER_NAME: &str = "vrclibman";
const BACKUP_SUFFIX: &str = ".vrclibman-backup";
const SKILL_MARKER: &str = ".vrclibman-skill";
const CLI_TIMEOUT: Duration = Duration::from_secs(20);

/// 設定ファイルの場所の基準。
#[derive(Debug, Clone, Copy)]
enum Base {
    Home,
    /// Windows は `%APPDATA%`、Mac は `~/Library/Application Support`、Linux は `~/.config`。
    AppConfig,
    /// Windows は `%LOCALAPPDATA%`、Mac は `~/Library/Application Support`、Linux は `~/.config`。
    LocalConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    /// `mcpServers.vrclibman = {command, args}`。`extra` の項目を足す。
    McpServers,
    /// VS Code: `servers.vrclibman = {type:"stdio", command, args}`。
    VsCode,
    /// Copilot CLI: `mcpServers.vrclibman = {type:"local", command, args, tools:["*"]}`。
    CopilotCli,
    /// OpenCode / Kilo: `mcp.vrclibman = {type:"local", command:[exe, "--mcp"]}`。
    OpenCodeLike,
    /// Codex: `[mcp_servers.vrclibman]`。
    CodexToml,
    /// Claude Code: `claude mcp add --scope user`。`~/.claude.json` は本体が書き換え続けるので触らない。
    ClaudeCli,
    /// 自動では書かない。手動用の内容だけを出す。
    Manual,
}

struct Client {
    id: &'static str,
    label: &'static str,
    format: Format,
    base: Base,
    /// 基準からの相対パス。`ClaudeCli` と `Manual` でも手動用の場所として使う。
    path: &'static [&'static str],
    /// `McpServers` の項目に足す値（Windsurf の `disabled` など）。
    extra: &'static [(&'static str, Extra)],
    /// ホームからの skill フォルダ。空なら SKILL を入れない。
    skills: &'static [&'static [&'static str]],
    /// 入っているかの目印（ホームからのフォルダ）。空なら設定ファイルの親フォルダで見る。
    detect: &'static [&'static [&'static str]],
    /// この PC で書いて動きを確かめたか。
    verified: bool,
    /// 手動の手順（`Manual` のとき）。
    note: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum Extra {
    False,
    EmptyArray,
    True,
}

impl Extra {
    fn value(self) -> Value {
        match self {
            Extra::False => Value::Bool(false),
            Extra::True => Value::Bool(true),
            Extra::EmptyArray => Value::Array(Vec::new()),
        }
    }
}

const AGENTS_SKILLS: &[&str] = &[".agents", "skills"];
const DISABLED_FALSE: &[(&str, Extra)] = &[("disabled", Extra::False)];

const CLIENTS: &[Client] = &[
    Client {
        id: "claude-code",
        label: "Claude Code",
        format: Format::ClaudeCli,
        base: Base::Home,
        path: &[".claude.json"],
        extra: &[],
        skills: &[&[".claude", "skills"]],
        detect: &[&[".claude"]],
        verified: true,
        note: "",
    },
    Client {
        id: "claude-desktop",
        label: "Claude Desktop",
        format: Format::McpServers,
        base: Base::AppConfig,
        path: &["Claude", "claude_desktop_config.json"],
        extra: &[],
        skills: &[],
        detect: &[],
        verified: true,
        note: "",
    },
    Client {
        id: "codex",
        label: "Codex",
        format: Format::CodexToml,
        base: Base::Home,
        path: &[".codex", "config.toml"],
        extra: &[],
        skills: &[AGENTS_SKILLS, &[".codex", "skills"]],
        detect: &[],
        verified: true,
        note: "",
    },
    Client {
        id: "cursor",
        label: "Cursor",
        format: Format::McpServers,
        base: Base::Home,
        path: &[".cursor", "mcp.json"],
        extra: &[],
        skills: &[&[".cursor", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "gemini-cli",
        label: "Gemini CLI",
        format: Format::McpServers,
        base: Base::Home,
        path: &[".gemini", "settings.json"],
        extra: &[],
        skills: &[&[".gemini", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "vscode",
        label: "VSCode GitHub Copilot",
        format: Format::VsCode,
        base: Base::AppConfig,
        path: &["Code", "User", "mcp.json"],
        extra: &[],
        skills: &[&[".copilot", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "vscode-insiders",
        label: "VSCode Insiders GitHub Copilot",
        format: Format::VsCode,
        base: Base::AppConfig,
        path: &["Code - Insiders", "User", "mcp.json"],
        extra: &[],
        skills: &[&[".copilot", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "copilot-cli",
        label: "GitHub Copilot CLI",
        format: Format::CopilotCli,
        base: Base::Home,
        path: &[".copilot", "mcp-config.json"],
        extra: &[],
        skills: &[&[".copilot", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "rider-copilot",
        label: "Rider GitHub Copilot",
        format: Format::VsCode,
        base: Base::LocalConfig,
        path: &["github-copilot", "intellij", "mcp.json"],
        extra: &[],
        skills: &[&[".copilot", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "windsurf",
        label: "Windsurf",
        format: Format::McpServers,
        base: Base::Home,
        path: &[".codeium", "windsurf", "mcp_config.json"],
        extra: DISABLED_FALSE,
        skills: &[AGENTS_SKILLS],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "cline",
        label: "Cline",
        format: Format::McpServers,
        base: Base::AppConfig,
        path: &[
            "Code",
            "User",
            "globalStorage",
            "saoudrizwan.claude-dev",
            "settings",
            "cline_mcp_settings.json",
        ],
        extra: &[
            ("disabled", Extra::False),
            ("autoApprove", Extra::EmptyArray),
        ],
        skills: &[&[".cline", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "kiro",
        label: "Kiro",
        format: Format::McpServers,
        base: Base::Home,
        path: &[".kiro", "settings", "mcp.json"],
        extra: DISABLED_FALSE,
        skills: &[&[".kiro", "skills"]],
        detect: &[&[".kiro"]],
        verified: false,
        note: "",
    },
    Client {
        id: "opencode",
        label: "OpenCode",
        format: Format::OpenCodeLike,
        base: Base::Home,
        path: &[".config", "opencode", "opencode.json"],
        extra: &[("enabled", Extra::True)],
        skills: &[&[".config", "opencode", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "kilo-code",
        label: "Kilo Code",
        format: Format::OpenCodeLike,
        base: Base::Home,
        path: &[".config", "kilo", "kilo.json"],
        extra: &[("enabled", Extra::True)],
        skills: &[&[".kilo", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "antigravity",
        label: "Antigravity 2.0",
        format: Format::McpServers,
        base: Base::Home,
        path: &[".gemini", "config", "mcp_config.json"],
        extra: DISABLED_FALSE,
        skills: &[&[".gemini", "config", "skills"]],
        detect: &[
            &[".antigravity"],
            &[".gemini", "config"],
            &[".gemini", "antigravity"],
        ],
        verified: false,
        note: "",
    },
    Client {
        id: "antigravity-ide",
        label: "Antigravity IDE",
        format: Format::McpServers,
        base: Base::Home,
        path: &[".gemini", "antigravity-ide", "mcp_config.json"],
        extra: DISABLED_FALSE,
        skills: &[&[".gemini", "config", "skills"]],
        detect: &[&[".gemini", "antigravity-ide"]],
        verified: false,
        note: "",
    },
    Client {
        id: "qwen-code",
        label: "Qwen Code",
        format: Format::McpServers,
        base: Base::Home,
        path: &[".qwen", "settings.json"],
        extra: &[],
        skills: &[&[".qwen", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "trae",
        label: "Trae",
        format: Format::McpServers,
        base: Base::AppConfig,
        path: &["Trae", "User", "mcp.json"],
        extra: &[],
        skills: &[&[".trae", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "trae-cn",
        label: "Trae CN",
        format: Format::McpServers,
        base: Base::AppConfig,
        path: &["Trae CN", "User", "mcp.json"],
        extra: &[],
        skills: &[&[".trae", "skills"]],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "codebuddy-cli",
        label: "CodeBuddy CLI",
        format: Format::McpServers,
        base: Base::Home,
        path: &[".codebuddy.json"],
        extra: &[],
        skills: &[],
        detect: &[&[".codebuddy"]],
        verified: false,
        note: "",
    },
    Client {
        id: "kimi-code",
        label: "Kimi Code",
        format: Format::McpServers,
        base: Base::Home,
        path: &[".kimi", "mcp.json"],
        extra: &[],
        skills: &[],
        detect: &[],
        verified: false,
        note: "",
    },
    Client {
        id: "cherry-studio",
        label: "Cherry Studio",
        format: Format::Manual,
        base: Base::AppConfig,
        path: &["Cherry Studio", "config"],
        extra: &[],
        skills: &[],
        detect: &[],
        verified: false,
        note: "Cherry Studio の設定 → MCP サーバー →「サーバーを追加」で、種類を stdio、コマンドを下の実行ファイル、引数を --mcp にして保存してください。",
    },
    Client {
        id: "openclaw",
        label: "OpenClaw",
        format: Format::Manual,
        base: Base::Home,
        path: &[".openclaw", "openclaw.json"],
        extra: &[],
        skills: &[],
        detect: &[&[".openclaw"]],
        verified: false,
        note: "OpenClaw は openclaw-mcp-bridge プラグイン経由で MCP を使います。プラグインの設定に下の内容を足してください。",
    },
    Client {
        id: "pi",
        label: "Pi",
        format: Format::Manual,
        base: Base::Home,
        path: &[".config", "mcp", "mcp.json"],
        extra: &[],
        skills: &[],
        detect: &[&[".pi", "agent"]],
        verified: false,
        note: "Pi には MCP の機能がありません。pi-mcp-adapter などの拡張を入れてから、下の内容を設定してください。",
    },
];

// ---- 場所 ----

/// 環境（ホームと各フォルダ）。テストでは一時フォルダに差し替える。
#[derive(Debug, Clone)]
pub(crate) struct Env {
    pub home: PathBuf,
    pub app_config: PathBuf,
    pub local_config: PathBuf,
    /// Windows のストア版（MSIX）の Claude Desktop が実際に読む Roaming フォルダ。
    pub claude_msix_roaming: Vec<PathBuf>,
}

impl Env {
    pub(crate) fn current() -> Option<Self> {
        let home = home_dir()?;
        #[cfg(windows)]
        let (app_config, local_config) = (
            std::env::var_os("APPDATA").map(PathBuf::from)?,
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?,
        );
        #[cfg(target_os = "macos")]
        let (app_config, local_config) = {
            let support = home.join("Library").join("Application Support");
            (support.clone(), support)
        };
        #[cfg(not(any(windows, target_os = "macos")))]
        let (app_config, local_config) = {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(".config"));
            (config.clone(), config)
        };
        let claude_msix_roaming = msix_roaming(&local_config);
        Some(Env {
            home,
            app_config,
            local_config,
            claude_msix_roaming,
        })
    }

    fn base(&self, base: Base) -> &Path {
        match base {
            Base::Home => &self.home,
            Base::AppConfig => &self.app_config,
            Base::LocalConfig => &self.local_config,
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    home.map(PathBuf::from).filter(|path| path.is_absolute())
}

/// `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming`（Windows だけ）。
fn msix_roaming(local: &Path) -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(local.join("Packages")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("Claude_"))
        .map(|entry| entry.path().join("LocalCache").join("Roaming"))
        .filter(|path| path.is_dir())
        .collect()
}

fn joined(base: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

/// 設定を書くファイル。Claude Desktop のストア版は、実際に読む場所を加える。
fn config_paths(env: &Env, client: &Client) -> Vec<PathBuf> {
    let primary = joined(env.base(client.base), client.path);
    if client.id != "claude-desktop" {
        return vec![primary];
    }
    let msix: Vec<PathBuf> = env
        .claude_msix_roaming
        .iter()
        .map(|roaming| joined(roaming, client.path))
        .filter(|path| path.parent().is_some_and(Path::is_dir))
        .collect();
    if msix.is_empty() {
        vec![primary]
    } else if primary.parent().is_some_and(Path::is_dir) {
        let mut all = msix;
        all.push(primary);
        all
    } else {
        msix
    }
}

fn installed(env: &Env, client: &Client) -> bool {
    if !client.detect.is_empty() {
        return client
            .detect
            .iter()
            .any(|parts| joined(&env.home, parts).is_dir());
    }
    match client.format {
        Format::ClaudeCli => find_on_path("claude").is_some(),
        _ => config_paths(env, client)
            .iter()
            .any(|path| path.parent().is_some_and(Path::is_dir)),
    }
}

// ---- 中身の組み立て ----

fn entry_value(client: &Client, exe: &str) -> Value {
    match client.format {
        Format::VsCode => json!({ "type": "stdio", "command": exe, "args": ["--mcp"] }),
        Format::CopilotCli => {
            json!({ "type": "local", "command": exe, "args": ["--mcp"], "tools": ["*"] })
        }
        Format::OpenCodeLike => {
            let mut value = json!({ "type": "local", "command": [exe, "--mcp"] });
            for (key, extra) in client.extra {
                value[*key] = extra.value();
            }
            value
        }
        _ => {
            let mut value = json!({ "command": exe, "args": ["--mcp"] });
            for (key, extra) in client.extra {
                value[*key] = extra.value();
            }
            value
        }
    }
}

fn container_key(format: Format) -> &'static str {
    match format {
        Format::VsCode => "servers",
        Format::OpenCodeLike => "mcp",
        _ => "mcpServers",
    }
}

fn toml_literal(text: &str) -> Option<String> {
    (!text.contains('\'') && !text.contains('\n')).then(|| format!("'{text}'"))
}

fn codex_block(exe: &str) -> Option<String> {
    Some(format!(
        "[mcp_servers.{SERVER_NAME}]\ncommand = {}\nargs = [\"--mcp\"]\n",
        toml_literal(exe)?
    ))
}

/// 手動で貼る内容。
fn manual_snippet(client: &Client, exe: &str) -> String {
    match client.format {
        Format::CodexToml => codex_block(exe).unwrap_or_default(),
        Format::ClaudeCli => {
            format!("claude mcp add --scope user {SERVER_NAME} -- \"{exe}\" --mcp")
        }
        _ => {
            let key = container_key(client.format);
            let mut inner = Map::new();
            inner.insert(SERVER_NAME.into(), entry_value(client, exe));
            let mut root = Map::new();
            root.insert(key.into(), Value::Object(inner));
            serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
        }
    }
}

// ---- JSON の結合 ----

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Missing,
    Object(Map<String, Value>),
    /// コメント付きなど、書き戻せない。
    Unreadable,
}

fn read_json(path: &Path) -> Result<Parsed, String> {
    match std::fs::read_to_string(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Parsed::Missing),
        Err(err) => Err(format!("設定ファイルを読めません: {err}")),
        Ok(text) => {
            let trimmed = text.trim_start_matches('\u{feff}').trim();
            if trimmed.is_empty() {
                return Ok(Parsed::Object(Map::new()));
            }
            Ok(match serde_json::from_str::<Value>(trimmed) {
                Ok(Value::Object(map)) => Parsed::Object(map),
                _ => Parsed::Unreadable,
            })
        }
    }
}

/// `root[key][SERVER_NAME]` を `entry` にする（`None` なら消す）。変わったら `true`。
fn merge_json(
    root: &mut Map<String, Value>,
    key: &str,
    entry: Option<Value>,
) -> Result<bool, String> {
    let container = root
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers) = container else {
        return Err(format!("設定ファイルの「{key}」が想定と違う形です"));
    };
    let changed = match entry {
        Some(entry) => {
            if servers.get(SERVER_NAME) == Some(&entry) {
                false
            } else {
                servers.insert(SERVER_NAME.into(), entry);
                true
            }
        }
        None => servers.shift_remove(SERVER_NAME).is_some(),
    };
    Ok(changed)
}

// ---- TOML（Codex）----

/// `[mcp_servers.vrclibman]` とその下位の表（`.env` など）を取り除いた本文。
fn toml_without_server(text: &str) -> (String, bool) {
    let header = format!("[mcp_servers.{SERVER_NAME}]");
    let sub = format!("[mcp_servers.{SERVER_NAME}.");
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    let mut removed = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            let ours = trimmed.starts_with(&header) || trimmed.starts_with(&sub);
            skipping = ours;
            removed |= ours;
        }
        if !skipping {
            out.push_str(line);
        }
    }
    (out, removed)
}

fn toml_with_server(text: &str, exe: &str) -> Result<String, String> {
    let block = codex_block(exe)
        .ok_or("実行ファイルのパスに「'」が含まれるため、Codex の設定に書けません")?;
    let (mut rest, _) = toml_without_server(text);
    if !rest.is_empty() && !rest.ends_with('\n') {
        rest.push('\n');
    }
    if !rest.is_empty() && !rest.ends_with("\n\n") {
        rest.push('\n');
    }
    rest.push_str(&block);
    Ok(rest)
}

fn toml_points_to(text: &str) -> Option<String> {
    let header = format!("[mcp_servers.{SERVER_NAME}]");
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == header;
            continue;
        }
        if inside && let Some(value) = trimmed.strip_prefix("command") {
            let value = value.trim_start().strip_prefix('=')?.trim();
            return Some(
                value
                    .trim_matches(|ch| ch == '\'' || ch == '"')
                    .replace("\\\\", "\\"),
            );
        }
    }
    None
}

// ---- ファイルの書き込み ----

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// 元のファイルをバックアップに写してから、一時ファイルを経由して置き換える。
fn write_with_backup(path: &Path, text: &str) -> Result<Option<PathBuf>, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "設定ファイルの場所が正しくありません".to_string())?;
    let backup = if path.exists() {
        let backup = backup_path(path);
        std::fs::copy(path, &backup).map_err(|err| format!("バックアップを作れません: {err}"))?;
        Some(backup)
    } else {
        None
    };
    let mut temp = tempfile::Builder::new()
        .prefix(".vrclibman-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|err| format!("設定ファイルを書けません: {err}"))?;
    use std::io::Write;
    temp.write_all(text.as_bytes())
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|err| format!("設定ファイルを書けません: {err}"))?;
    temp.persist(path)
        .map_err(|err| format!("設定ファイルを置き換えられません: {}", err.error))?;
    Ok(backup)
}

// ---- 状態 ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ConfigState {
    None,
    Current,
    OtherPath,
    Unreadable,
    /// 自動では書かない（手動用）。
    Manual,
    /// Claude Code の CLI が見つからない、などで確かめられない。
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SkillState {
    Unsupported,
    None,
    Current,
    Outdated,
    /// 同名のフォルダを利用者が作っている。触らない。
    Foreign,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClientStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub installed: bool,
    pub verified: bool,
    pub automatic: bool,
    pub config: ConfigState,
    pub skill: SkillState,
    /// 手動用に見せる設定ファイルの場所。
    pub config_path: String,
    pub manual: String,
    pub note: &'static str,
}

fn same_exe(found: &str, exe: &str) -> bool {
    if cfg!(windows) {
        found.eq_ignore_ascii_case(exe)
    } else {
        found == exe
    }
}

fn config_state(env: &Env, client: &Client, exe: &str) -> ConfigState {
    match client.format {
        Format::Manual => ConfigState::Manual,
        Format::ClaudeCli => claude_cli_state(exe),
        Format::CodexToml => {
            let path = joined(env.base(client.base), client.path);
            match std::fs::read_to_string(&path) {
                Err(_) => ConfigState::None,
                Ok(text) => match toml_points_to(&text) {
                    None => ConfigState::None,
                    Some(found) if same_exe(&found, exe) => ConfigState::Current,
                    Some(_) => ConfigState::OtherPath,
                },
            }
        }
        _ => {
            let key = container_key(client.format);
            let mut state = ConfigState::None;
            for path in config_paths(env, client) {
                let found = match read_json(&path) {
                    Ok(Parsed::Object(root)) => root
                        .get(key)
                        .and_then(|servers| servers.get(SERVER_NAME))
                        .map(|entry| {
                            let command = match &entry["command"] {
                                Value::Array(parts) => {
                                    parts.first().cloned().unwrap_or(Value::Null)
                                }
                                other => other.clone(),
                            };
                            command.as_str().is_some_and(|found| same_exe(found, exe))
                        }),
                    Ok(Parsed::Missing) => None,
                    Ok(Parsed::Unreadable) | Err(_) => return ConfigState::Unreadable,
                };
                state = match (state, found) {
                    (_, Some(false)) => ConfigState::OtherPath,
                    (ConfigState::OtherPath, _) => ConfigState::OtherPath,
                    (_, Some(true)) => ConfigState::Current,
                    (state, None) => state,
                };
            }
            state
        }
    }
}

fn skill_dirs(env: &Env, client: &Client) -> Vec<PathBuf> {
    client
        .skills
        .iter()
        .map(|parts| joined(&env.home, parts).join(SERVER_NAME))
        .collect()
}

fn skill_state(env: &Env, client: &Client, version: &str) -> SkillState {
    let dirs = skill_dirs(env, client);
    if dirs.is_empty() {
        return SkillState::Unsupported;
    }
    let mut state = SkillState::Current;
    for dir in dirs {
        let one = if !dir.exists() {
            SkillState::None
        } else {
            match std::fs::read_to_string(dir.join(SKILL_MARKER)) {
                Err(_) => SkillState::Foreign,
                Ok(text) if text.trim() == version => SkillState::Current,
                Ok(_) => SkillState::Outdated,
            }
        };
        state = match (state, one) {
            (SkillState::Foreign, _) | (_, SkillState::Foreign) => SkillState::Foreign,
            (SkillState::None, _) | (_, SkillState::None) => SkillState::None,
            (SkillState::Outdated, _) | (_, SkillState::Outdated) => SkillState::Outdated,
            _ => SkillState::Current,
        };
    }
    state
}

pub(crate) fn statuses(env: &Env, exe: &str, version: &str) -> Vec<ClientStatus> {
    CLIENTS
        .iter()
        .map(|client| ClientStatus {
            id: client.id,
            label: client.label,
            installed: installed(env, client),
            verified: client.verified,
            automatic: client.format != Format::Manual,
            config: config_state(env, client, exe),
            skill: skill_state(env, client, version),
            config_path: config_paths(env, client)
                .first()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            manual: manual_snippet(client, exe),
            note: client.note,
        })
        .collect()
}

// ---- 操作 ----

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeReport {
    pub label: &'static str,
    /// 書き換えたファイル。
    pub changed: Vec<String>,
    /// 取ったバックアップ。
    pub backups: Vec<String>,
}

fn find(id: &str) -> Result<&'static Client, String> {
    CLIENTS
        .iter()
        .find(|client| client.id == id)
        .ok_or_else(|| "知らないエージェントです".to_string())
}

/// 設定を書く（`add`）か外す。
pub(crate) fn change_config(
    env: &Env,
    id: &str,
    exe: &str,
    add: bool,
) -> Result<ChangeReport, String> {
    let client = find(id)?;
    let mut report = ChangeReport {
        label: client.label,
        changed: Vec::new(),
        backups: Vec::new(),
    };
    match client.format {
        Format::Manual => {
            return Err(format!(
                "{} は自動で設定できません。「手動で設定する」の内容を使ってください",
                client.label
            ));
        }
        Format::ClaudeCli => {
            claude_cli_change(exe, add)?;
            report.changed.push("claude mcp".into());
            return Ok(report);
        }
        Format::CodexToml => {
            let path = joined(env.base(client.base), client.path);
            if !path.parent().is_some_and(Path::is_dir) {
                return Err(not_installed(client));
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(err) => return Err(format!("設定ファイルを読めません: {err}")),
            };
            let next = if add {
                toml_with_server(&text, exe)?
            } else {
                toml_without_server(&text).0
            };
            if next != text {
                if let Some(backup) = write_with_backup(&path, &next)? {
                    report.backups.push(backup.display().to_string());
                }
                report.changed.push(path.display().to_string());
            }
            return Ok(report);
        }
        _ => {}
    }
    let key = container_key(client.format);
    let paths: Vec<PathBuf> = config_paths(env, client)
        .into_iter()
        .filter(|path| path.parent().is_some_and(Path::is_dir))
        .collect();
    if paths.is_empty() {
        return Err(not_installed(client));
    }
    // 先にすべて読んで、書けないものがあれば何も書かない。
    let mut plans = Vec::new();
    for path in paths {
        let mut root = match read_json(&path)? {
            Parsed::Object(root) => root,
            Parsed::Missing if add => Map::new(),
            Parsed::Missing => continue,
            Parsed::Unreadable => {
                return Err(format!(
                    "{} の設定ファイルにコメントなどがあり、内容を保ったまま書き換えられません。「手動で設定する」の内容を貼ってください",
                    client.label
                ));
            }
        };
        let entry = add.then(|| entry_value(client, exe));
        if !add && !root.contains_key(key) {
            continue;
        }
        if merge_json(&mut root, key, entry)? {
            plans.push((path, root));
        }
    }
    for (path, root) in plans {
        let text = serde_json::to_string_pretty(&Value::Object(root))
            .map_err(|err| format!("設定を組み立てられません: {err}"))?
            + "\n";
        if let Some(backup) = write_with_backup(&path, &text)? {
            report.backups.push(backup.display().to_string());
        }
        report.changed.push(path.display().to_string());
    }
    Ok(report)
}

fn not_installed(client: &Client) -> String {
    format!(
        "{} が見つかりません。インストールして一度起動してから、もう一度お試しください",
        client.label
    )
}

/// 同梱の skill（`source` の中身）を写すか外す。印のあるフォルダだけを書き換える。
pub(crate) fn change_skill(
    env: &Env,
    id: &str,
    source: &Path,
    version: &str,
    add: bool,
) -> Result<ChangeReport, String> {
    let client = find(id)?;
    let dirs = skill_dirs(env, client);
    if dirs.is_empty() {
        return Err(format!("{} は SKILL に対応していません", client.label));
    }
    let mut report = ChangeReport {
        label: client.label,
        changed: Vec::new(),
        backups: Vec::new(),
    };
    for dir in &dirs {
        if dir.exists() && !dir.join(SKILL_MARKER).is_file() {
            return Err(format!(
                "{} に、VRCLibMan が入れたものではない同じ名前のフォルダがあります。書き換えないので、不要なら自分で移してください",
                dir.display()
            ));
        }
    }
    for dir in dirs {
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|err| format!("SKILL を消せません: {err}"))?;
            if !add {
                report.changed.push(dir.display().to_string());
            }
        }
        if add {
            copy_tree(source, &dir)?;
            std::fs::write(dir.join(SKILL_MARKER), version)
                .map_err(|err| format!("SKILL を入れられません: {err}"))?;
            report.changed.push(dir.display().to_string());
        }
    }
    Ok(report)
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|err| format!("SKILL を入れられません: {err}"))?;
    let entries =
        std::fs::read_dir(from).map_err(|err| format!("同梱の SKILL が見つかりません: {err}"))?;
    for entry in entries.flatten() {
        let kind = entry
            .file_type()
            .map_err(|err| format!("同梱の SKILL を読めません: {err}"))?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)
                .map_err(|err| format!("SKILL を入れられません: {err}"))?;
        }
    }
    Ok(())
}

// ---- Claude Code の CLI ----

fn find_on_path(name: &str) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd"]
            .iter()
            .map(|ext| format!("{name}.{ext}"))
            .collect()
    } else {
        vec![name.to_string()]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    // 既定のインストール先（GUI から起動したときは PATH に入っていないことがある）。
    if let Some(home) = home_dir() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".claude").join("local"));
    }
    dirs.iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|path| path.is_file())
}

fn run_cli(args: &[&str]) -> Result<std::process::Output, String> {
    let program = find_on_path("claude").ok_or_else(|| {
        "Claude Code（claude コマンド）が見つかりません。「手動で設定する」のコマンドをターミナルで実行してください".to_string()
    })?;
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let child = command
        .spawn()
        .map_err(|err| format!("claude コマンドを実行できません: {err}"))?;
    crate::library_files::wait_for(CLI_TIMEOUT, move || child.wait_with_output())
        .ok_or_else(|| "claude コマンドが応答しません".to_string())?
        .map_err(|err| format!("claude コマンドを実行できません: {err}"))
}

fn claude_cli_state(exe: &str) -> ConfigState {
    if find_on_path("claude").is_none() {
        return ConfigState::Unknown;
    }
    match run_cli(&["mcp", "get", SERVER_NAME]) {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let normalized = |value: &str| value.replace('\\', "/").to_lowercase();
            if normalized(&text).contains(&normalized(exe)) {
                ConfigState::Current
            } else {
                ConfigState::OtherPath
            }
        }
        Ok(_) => ConfigState::None,
        Err(_) => ConfigState::Unknown,
    }
}

fn claude_cli_change(exe: &str, add: bool) -> Result<(), String> {
    // 前の登録を外してから足す。登録が無いときの失敗は気にしない。
    let _ = run_cli(&["mcp", "remove", "--scope", "user", SERVER_NAME]);
    if !add {
        return Ok(());
    }
    let output = run_cli(&[
        "mcp",
        "add",
        "--scope",
        "user",
        SERVER_NAME,
        "--",
        exe,
        "--mcp",
    ])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Claude Code に登録できませんでした: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Apps\VRCLibMan\VRCLibMan.exe";

    fn env(root: &Path) -> Env {
        let home = root.join("home");
        let app = root.join("appdata");
        let local = root.join("local");
        for dir in [&home, &app, &local] {
            std::fs::create_dir_all(dir).unwrap();
        }
        Env {
            home,
            app_config: app,
            local_config: local,
            claude_msix_roaming: Vec::new(),
        }
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn json_merge_keeps_other_servers_and_order_and_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path());
        let path = env.home.join(".cursor").join("mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"zeta":1,"mcpServers":{"unityMCP":{"command":"uvx","args":["x"]}},"alpha":true}"#,
        )
        .unwrap();
        let report = change_config(&env, "cursor", EXE, true).unwrap();
        assert_eq!(report.backups.len(), 1);
        let backup = std::fs::read_to_string(&report.backups[0]).unwrap();
        assert!(backup.contains("unityMCP") && !backup.contains("vrclibman"));
        let value = read(&path);
        let keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["zeta", "mcpServers", "alpha"], "並びを保つ");
        assert_eq!(value["mcpServers"]["unityMCP"]["command"], "uvx");
        assert_eq!(value["mcpServers"]["vrclibman"]["command"], EXE);
        assert_eq!(value["mcpServers"]["vrclibman"]["args"][0], "--mcp");

        let before = std::fs::read_to_string(&path).unwrap();
        let again = change_config(&env, "cursor", EXE, true).unwrap();
        assert!(again.changed.is_empty(), "二度目は書かない");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        let statuses = statuses(&env, EXE, "1");
        let cursor = statuses.iter().find(|s| s.id == "cursor").unwrap();
        assert!(cursor.installed);
        assert_eq!(cursor.config, ConfigState::Current);
        let other = statuses_for(&env, "cursor", r"D:\Other.exe");
        assert_eq!(other, ConfigState::OtherPath);

        change_config(&env, "cursor", EXE, false).unwrap();
        let value = read(&path);
        assert!(value["mcpServers"].get("vrclibman").is_none());
        assert_eq!(value["mcpServers"]["unityMCP"]["command"], "uvx");
    }

    fn statuses_for(env: &Env, id: &str, exe: &str) -> ConfigState {
        config_state(env, find(id).unwrap(), exe)
    }

    #[test]
    fn missing_folder_means_not_installed_and_nothing_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path());
        let error = change_config(&env, "kiro", EXE, true).unwrap_err();
        assert!(error.contains("見つかりません"));
        assert!(!env.home.join(".kiro").exists());
    }

    #[test]
    fn jsonc_with_comments_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path());
        let path = env.app_config.join("Code").join("User").join("mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "{\n  // mine\n  \"servers\": {},\n}\n";
        std::fs::write(&path, original).unwrap();
        assert!(change_config(&env, "vscode", EXE, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(!backup_path(&path).exists());
        assert_eq!(statuses_for(&env, "vscode", EXE), ConfigState::Unreadable);
    }

    #[test]
    fn formats_follow_each_client() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path());
        for parts in [
            &["Code", "User"][..],
            &[".copilot"][..],
            &[".config", "opencode"][..],
            &[".codeium", "windsurf"][..],
        ] {
            let base = if parts[0] == "Code" {
                &env.app_config
            } else {
                &env.home
            };
            std::fs::create_dir_all(joined(base, parts)).unwrap();
        }
        change_config(&env, "vscode", EXE, true).unwrap();
        let vscode = read(&env.app_config.join("Code").join("User").join("mcp.json"));
        assert_eq!(vscode["servers"]["vrclibman"]["type"], "stdio");
        change_config(&env, "copilot-cli", EXE, true).unwrap();
        let copilot = read(&env.home.join(".copilot").join("mcp-config.json"));
        assert_eq!(copilot["mcpServers"]["vrclibman"]["type"], "local");
        assert_eq!(copilot["mcpServers"]["vrclibman"]["tools"][0], "*");
        change_config(&env, "opencode", EXE, true).unwrap();
        let opencode = read(
            &env.home
                .join(".config")
                .join("opencode")
                .join("opencode.json"),
        );
        assert_eq!(opencode["mcp"]["vrclibman"]["command"][1], "--mcp");
        assert_eq!(opencode["mcp"]["vrclibman"]["type"], "local");
        assert_eq!(statuses_for(&env, "opencode", EXE), ConfigState::Current);
        change_config(&env, "windsurf", EXE, true).unwrap();
        let windsurf = read(
            &env.home
                .join(".codeium")
                .join("windsurf")
                .join("mcp_config.json"),
        );
        assert_eq!(windsurf["mcpServers"]["vrclibman"]["disabled"], false);
        assert!(change_config(&env, "cherry-studio", EXE, true).is_err());
    }

    #[test]
    fn codex_toml_replaces_only_our_tables() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path());
        let path = env.home.join(".codex").join("config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "model = \"x\"\n\n[mcp_servers.unityMCP]\ncommand = \"uvx\"\n\n[mcp_servers.vrclibman]\ncommand = 'old'\n\n[mcp_servers.vrclibman.env]\nA = \"1\"\n\n[windows]\nsandbox = true\n";
        std::fs::write(&path, original).unwrap();
        change_config(&env, "codex", EXE, true).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[mcp_servers.unityMCP]\ncommand = \"uvx\""));
        assert!(text.contains("[windows]\nsandbox = true"));
        assert!(!text.contains("'old'") && !text.contains("A = \"1\""));
        assert_eq!(text.matches("[mcp_servers.vrclibman]").count(), 1);
        assert!(text.contains(&format!("command = '{EXE}'")));
        assert_eq!(statuses_for(&env, "codex", EXE), ConfigState::Current);
        assert_eq!(
            std::fs::read_to_string(backup_path(&path)).unwrap(),
            original
        );
        change_config(&env, "codex", EXE, false).unwrap();
        let removed = std::fs::read_to_string(&path).unwrap();
        assert!(!removed.contains("vrclibman"));
        assert!(removed.contains("[windows]") && removed.contains("model = \"x\""));
        assert_eq!(statuses_for(&env, "codex", EXE), ConfigState::None);
    }

    #[test]
    fn claude_desktop_writes_where_the_store_app_reads() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = env(dir.path());
        let roaming = dir.path().join("pkg").join("Roaming");
        std::fs::create_dir_all(roaming.join("Claude")).unwrap();
        env.claude_msix_roaming = vec![roaming.clone()];
        change_config(&env, "claude-desktop", EXE, true).unwrap();
        let written = read(&roaming.join("Claude").join("claude_desktop_config.json"));
        assert_eq!(written["mcpServers"]["vrclibman"]["command"], EXE);
        assert!(!env.app_config.join("Claude").exists(), "無い方は作らない");
        std::fs::create_dir_all(env.app_config.join("Claude")).unwrap();
        let report = change_config(&env, "claude-desktop", EXE, true).unwrap();
        assert_eq!(report.changed.len(), 1, "両方あれば両方。片方は済み");
    }

    #[test]
    fn skills_only_touch_marked_folders() {
        let dir = tempfile::tempdir().unwrap();
        let env = env(dir.path());
        let source = dir.path().join("bundle");
        std::fs::create_dir_all(source.join("references")).unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: vrclibman\n---\n").unwrap();
        std::fs::write(source.join("references").join("workflow.md"), "w").unwrap();
        let client = find("codex").unwrap();
        assert_eq!(skill_state(&env, client, "1.0.0"), SkillState::None);
        change_skill(&env, "codex", &source, "1.0.0", true).unwrap();
        let installed = env.home.join(".agents").join("skills").join("vrclibman");
        assert!(installed.join("references").join("workflow.md").is_file());
        assert!(
            env.home
                .join(".codex")
                .join("skills")
                .join("vrclibman")
                .join("SKILL.md")
                .is_file()
        );
        assert_eq!(skill_state(&env, client, "1.0.0"), SkillState::Current);
        assert_eq!(skill_state(&env, client, "1.1.0"), SkillState::Outdated);
        // 同じフォルダを共有するエージェントにも入っている。
        assert_eq!(
            skill_state(&env, find("windsurf").unwrap(), "1.0.0"),
            SkillState::Current
        );

        let mine = env.home.join(".cursor").join("skills").join("vrclibman");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(mine.join("SKILL.md"), "user's own").unwrap();
        assert_eq!(
            skill_state(&env, find("cursor").unwrap(), "1.0.0"),
            SkillState::Foreign
        );
        assert!(change_skill(&env, "cursor", &source, "1.0.0", true).is_err());
        assert!(change_skill(&env, "cursor", &source, "1.0.0", false).is_err());
        assert_eq!(
            std::fs::read_to_string(mine.join("SKILL.md")).unwrap(),
            "user's own"
        );

        change_skill(&env, "codex", &source, "1.0.0", false).unwrap();
        assert!(!installed.exists());
        assert!(change_skill(&env, "claude-desktop", &source, "1.0.0", true).is_err());
    }

    #[test]
    fn every_client_has_a_manual_snippet_and_unique_id() {
        let mut ids = std::collections::HashSet::new();
        for client in CLIENTS {
            assert!(ids.insert(client.id), "{}", client.id);
            assert!(!manual_snippet(client, EXE).is_empty(), "{}", client.id);
        }
        assert_eq!(CLIENTS.len(), 24);
        assert!(codex_block("C:\\it's\\x.exe").is_none());
    }
}
