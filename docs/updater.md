# 配布とアプリ内更新

配布担当向けの手順と更新の技術仕様です。利用者の更新操作は [使い方ガイド](../how-to-use.md#対応表とバージョン)、ビルド環境は [開発ガイド](development.md) を参照してください。

Windows x64 の NSIS 配布版と Apple Silicon Mac の .app が対象です。設定の「データ → アプリの更新」から、更新確認、ダウンロード、再起動して適用を順に実行します。起動時・定期的な外部通信や、無断のダウンロード・再起動はしません。画面を移動・閉じても、起動中のプロセス内に更新状態を保持します。アプリ自体を終了した場合は、再度確認・ダウンロードが必要です。

更新先は `https://github.com/KeisXR/VRCLibMan/releases/latest/download/latest.json` です。アプリ内更新は公開リポジトリから認証なしで取得する配布方式を前提にします。GitHub のトークンをアプリへ埋め込まないでください。

## 配布を始める前の設定

更新の署名鍵は、Windows / Apple のコード署名とは別です。コード署名・公証がない現在の配布方式でも、更新ファイルの改ざんは検証できます。

1. 開発者の端末で、リポジトリの外に署名鍵を一度だけ生成します。CLI が求めるパスワードを設定してください。鍵とパスワードを安全な別の場所にも保管してください。

   ```sh
   npx tauri signer generate --write-keys /安全な保存先/vrclibman-updater.key
   ```

2. GitHub リポジトリの Settings → Secrets and variables → Actions に登録します。

   | 種類 | 名前 | 内容 |
   | --- | --- | --- |
   | Secret | `TAURI_SIGNING_PRIVATE_KEY` | `.key` ファイルの内容 |
   | Secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 生成時のパスワード |
   | Variable | `TAURI_UPDATER_PUBLIC_KEY` | `.key.pub` ファイルの内容 |

   Secrets を操作できる GitHub CLI で登録する場合は、値をコマンド行へ貼らずに渡します。

   ```sh
   gh secret set TAURI_SIGNING_PRIVATE_KEY --repo KeisXR/VRCLibMan < /安全な保存先/vrclibman-updater.key
   gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo KeisXR/VRCLibMan
   gh variable set TAURI_UPDATER_PUBLIC_KEY --repo KeisXR/VRCLibMan < /安全な保存先/vrclibman-updater.key.pub
   ```

3. release ワークフローを「Run workflow」で試します。Artifacts の `updater-windows-x64` / `updater-macos-aarch64` に更新ファイル・`.sig`・検証用の公開鍵を保存します。タグも Release も作りません。
4. 次の「版を公開する」に従って、署名付きの配布物を作ります。

署名設定がない、または鍵が一致しない場合はリリースを失敗させます。公開済み Release の上書きはしません。公開鍵は CI の追加設定を通してアプリへ埋め込み、秘密鍵は署名工程だけに渡します。開発ビルド・通常の `cargo build` は、公開鍵を渡さなければアプリ内更新を無効として表示します。

配布ファイルのビルドが成功し、更新情報だけを再生成したい場合は、Actions の `finalize updater manifest` を手動実行し、既存のタグ（例: `v0.1.8`）を指定します。main に含まれるタグの下書きだけが対象です。両 OS の配布ファイルと署名を確認して `latest.json` を載せ直し、配布ファイルの再ビルド・タグの変更・Release の公開は行いません。

既に配布したアプリは埋め込まれた公開鍵で将来の更新を検証します。鍵を紛失・変更すると従来のアプリから更新できなくなるため、毎回生成し直さないでください。更新ファイルの署名には版も含め、古い署名付きファイルに新しい版番号を付ける差し替えを拒否します。

## 版を公開する

1. アプリの版を `package.json`、`package-lock.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` で揃えて上げます。拡張の版は `extension/manifest.json` で独立して管理し、拡張を変更したときに上げます。
2. `.github/release-notes/<アプリ版>.md` にその版の利用者向け変更を記載し、main にマージします。版別ファイルがなければリリース本文の生成は失敗します。
3. main に含まれるコミットへ `v<版>` タグを付けて push します。`.github/workflows/release.yml` が下書き Release を作り、Windows NSIS、Apple Silicon dmg、拡張 zip を載せます。
4. 両 OS のビルドと最後の `publish complete updater manifest` の成功後に、インストーラ、dmg、更新用 `.app.tar.gz`、対応する `.sig`、拡張 zip、`latest.json` と本文を確認します。`latest.json` には `windows-x86_64` と `darwin-aarch64` の両方が必要です。
5. 実機確認の結果を記録し、下書きの「Publish release」で公開します。

タグとアプリの版が違う、版が揃っていない、タグのコミットが main に含まれない、同じタグの Release が公開済みの場合は、ビルド前に失敗します。「Run workflow」はタグも Release も作らない試しビルドで、成果物は Artifacts に保存します。

下書き Release の `browser_download_url` は `untagged-*` になる場合があるため、更新情報には公開後の `v<版>` の URL を生成します。更新情報だけの再生成は、上記の `finalize updater manifest` を使います。署名済みの配布ファイル、タグ、公開状態は変更しません。

## 更新の流れと失敗時の動作

- 最新の公開済み通常 Release のみを取得します。下書きや prerelease は対象外です。同じ版・古い版へは更新しません。
- ダウンロード完了後に Tauri の公開鍵で署名と版を検証します。失敗したファイルは適用できず、再ダウンロードできます。更新内容は文字列として表示します。
- 「再起動して更新」を押し、確認すると、未保存の設定を保存してから適用します。分類・ライブラリインポート・BOOTH のダウンロード中は適用を拒否します。画面に表示されたファイル処理中もボタンを無効にします。
- Windows は NSIS の更新モードでインストーラを起動し、現在のプロセスを終了します。インストール後は NSIS が再起動します。Mac は .app を置き換えてからアプリを再起動します。Mac は `/Applications` など書き込み可能な場所へインストールして使ってください。dmg 内からの実行は更新に失敗することがあります。
- ライブラリ DB、設定、ダウンロード先のフォルダは既存のまま使用します。適用に失敗しても準備済みファイルから再試行できます。解決しない場合は「リリース一覧」から手動でインストールできます。
- 拡張 zip はブラウザの拡張管理画面で別途更新します。

## 検証

`npm test` は UI の明示操作、再起動確認の取消、設定保存エラー、再試行、リリース情報生成を検証します。Rust テストは、実際に Tauri CLI で署名した短いテストファイルで、改ざん拒否・版の差し替え拒否・再試行を検証します。テスト鍵は配布には使いません。

Windows / Mac での実インストーラによる更新は、署名付きの次版を使って確認し、対象コミット・OS・実施日・結果を記録します。自動テストの成功だけで、実インストーラによる更新まで確認済みと扱わないでください。記録方法は [検証記録](verification.md) にあります。アップデータを持たない v0.1.7 以前には、対応版を一度手動でインストールする案内が必要です。
