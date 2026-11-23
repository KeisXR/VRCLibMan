# AI エージェントの設定と SKILL

設定 → Unity →「AI エージェント連携」をオンにすると出る「AI エージェントへの登録」。MCP for Unity の Client Configuration と同じ一覧から選び、そのエージェントの設定ファイルに `vrclibman` を書き込み、使い方の SKILL を配る。実装は `src-tauri/src/agent_setup.rs`、画面は `src/components/AgentClients.tsx`。

## 書き込みの決まり

- 書くのは、表にある設定ファイルの `vrclibman` の項目だけ。ほかのキーとサーバーは、並びも含めてそのまま残す（`serde_json` の `preserve_order`）。
- 書く前に、元のファイルを `<ファイル名>.vrclibman-backup` に写す（毎回上書き）。そのあと同じフォルダの一時ファイルに書いて置き換える。同じ内容なら書かない。
- 設定ファイルの親フォルダが無ければ、そのエージェントは入っていないとみなし、フォルダを作らない。
- JSON として読めないもの（コメントや末尾のカンマがある JSONC）は書き戻さず、「手動で設定する」を案内する。
- 「外す」は `vrclibman` の項目だけを消す。
- Claude Code は本体が `~/.claude.json` を書き換え続けるので、ファイルは触らず `claude mcp remove/add --scope user` を引数の配列で実行する（シェルを通さない）。`claude` が見つからなければ、実行するコマンドを手動用に出す。
- Claude Desktop（Windows のストア版・MSIX）は、`%APPDATA%` への読み書きが `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming` に振り替えられる。そのフォルダがあればこちらに書き、通常の `%APPDATA%\Claude` もあれば両方に書く。
- Codex の TOML は `[mcp_servers.vrclibman]` とその下位の表（`.env` など）を取り除いてから、末尾に足し直す。パスはリテラル文字列 `'...'`。`'` を含むパスは書かない。
- 記録はエージェント名と、足したか外したかだけ。ネットワークには出ない。
- 「AI エージェント連携」がオフのときは書かない（コマンドが断る）。

## SKILL

- 同梱の `src-tauri/agent-skill/vrclibman/`（`SKILL.md` と `references/workflow.md`）を写すだけ。ネットからは取らない。
- 配る先は `<skill フォルダ>/vrclibman/`。印のファイル `.vrclibman-skill`（アプリの版）を置き、上書きと削除は印のあるフォルダだけ。同名のフォルダを利用者が作っていれば触らない（「同じ名前の別の SKILL があります」）。
- `~/.agents/skills` のように複数のエージェントが読むフォルダは、一度入れれば共有する。印の版がアプリより古ければ「SKILL を更新」を出す。自動では書き換えない。

## 対応表

「確認」は、この PC（Windows）で実際に書き込み、エージェントが読むところまで確かめたもの。ほかは unity-mcp（CoplayDev/unity-mcp の `MCPForUnity/Editor/Clients/Configurators`）と各ドキュメントに合わせたもので、画面に「未確認」と出す。`~` はホーム、AppConfig は Windows の `%APPDATA%`・Mac の `~/Library/Application Support`・Linux の `~/.config`。

| エージェント | 設定ファイル | 書式 | SKILL | 確認 |
| --- | --- | --- | --- | --- |
| Claude Code | `claude mcp add --scope user` | CLI | `~/.claude/skills` | 済 |
| Claude Desktop | AppConfig`/Claude/claude_desktop_config.json`（MSIX は上記） | `mcpServers` | なし | 済 |
| Codex | `~/.codex/config.toml` | `[mcp_servers.vrclibman]` | `~/.agents/skills`、`~/.codex/skills` | 済 |
| Cursor | `~/.cursor/mcp.json` | `mcpServers` | `~/.cursor/skills` | |
| Gemini CLI | `~/.gemini/settings.json` | `mcpServers` | `~/.gemini/skills` | |
| VS Code / Insiders | AppConfig`/Code[ - Insiders]/User/mcp.json` | `servers`（`type:"stdio"`） | `~/.copilot/skills` | |
| GitHub Copilot CLI | `~/.copilot/mcp-config.json` | `mcpServers`（`type:"local"`、`tools:["*"]`） | `~/.copilot/skills` | |
| Rider GitHub Copilot | Windows `%LOCALAPPDATA%`・ほか AppConfig の `github-copilot/intellij/mcp.json` | `servers` | `~/.copilot/skills` | |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` | `mcpServers`（`disabled:false`） | `~/.agents/skills` | |
| Cline | AppConfig`/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json` | `mcpServers`（`disabled`、`autoApprove`） | `~/.cline/skills` | |
| Kiro | `~/.kiro/settings/mcp.json` | `mcpServers`（`disabled:false`） | `~/.kiro/skills` | |
| OpenCode | `~/.config/opencode/opencode.json` | `mcp.vrclibman`（`type:"local"`、`command` は配列） | `~/.config/opencode/skills` | |
| Kilo Code | `~/.config/kilo/kilo.json` | `mcp.vrclibman`（同上） | `~/.kilo/skills` | |
| Antigravity 2.0 | `~/.gemini/config/mcp_config.json` | `mcpServers`（`disabled:false`） | `~/.gemini/config/skills` | |
| Antigravity IDE | `~/.gemini/antigravity-ide/mcp_config.json` | 同上 | 同上 | |
| Qwen Code | `~/.qwen/settings.json` | `mcpServers` | `~/.qwen/skills` | |
| Trae / Trae CN | AppConfig`/Trae[ CN]/User/mcp.json` | `mcpServers` | `~/.trae/skills` | |
| CodeBuddy CLI | `~/.codebuddy.json` | `mcpServers` | なし | |
| Kimi Code | `~/.kimi/mcp.json` | `mcpServers` | なし | |
| Cherry Studio | アプリの画面 | 手動 | なし | |
| OpenClaw | `~/.openclaw/openclaw.json`（openclaw-mcp-bridge） | 手動 | なし | |
| Pi | `~/.config/mcp/mcp.json`（pi-mcp-adapter） | 手動 | なし | |

OpenCode の新しいドキュメントは `mcp.servers.<名前>` の形を示しているが、この PC の OpenCode の設定は `mcp.<名前>` で動いているので、こちらに合わせた。
