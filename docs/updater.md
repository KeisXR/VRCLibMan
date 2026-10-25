# アプリ内更新

Windows x64 の NSIS 配布版と Apple Silicon Mac の .app が対象です。設定の「データ → アプリの更新」から、更新確認、ダウンロード、再起動して適用を順に実行します。起動時・定期的な外部通信や、無断のダウンロード・再起動はしません。画面を移動・閉じても、起動中のプロセス内に更新状態を保持します。アプリ自体を終了した場合は、再度確認・ダウンロードが必要です。

更新先は `https://github.com/KeisXR/VRCLibMan/releases/latest/download/latest.json` です。本体リポジトリを公開してから、認証なしで利用できます。非公開の間は手動で Releases からダウンロードします。GitHub のトークンをアプリへ埋め込む必要はありません。

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

3. release ワークフローを「Run workflow」で試します。署名付きの更新ファイルと `.sig` を Artifacts で確認できます。タグも Release も作りません。
4. 通常どおり版を上げて main にマージし、`v<版>` タグを push します。Windows・Mac 両方のビルドと、最後の `publish complete updater manifest` が成功するまで待ちます。
5. 下書き Release のインストーラ、dmg、`.app.tar.gz`、対応する `.sig`、拡張 zip、`latest.json` を確認して公開します。`latest.json` には `windows-x86_64` と `darwin-aarch64` の両方が必要です。

署名設定がない、または鍵が一致しない場合はリリースを失敗させます。公開済み Release の上書きはしません。公開鍵は CI の追加設定を通してアプリへ埋め込み、秘密鍵は署名工程だけに渡します。開発ビルド・通常の `cargo build` は、公開鍵を渡さなければアプリ内更新を無効として表示します。

既に配布したアプリは埋め込まれた公開鍵で将来の更新を検証します。鍵を紛失・変更すると従来のアプリから更新できなくなるため、毎回生成し直さないでください。更新ファイルの署名には版も含め、古い署名付きファイルに新しい版番号を付ける差し替えを拒否します。

## 更新の流れと失敗時の動作

- 最新の公開済み通常 Release のみを取得します。下書きや prerelease は対象外です。同じ版・古い版へは更新しません。
- ダウンロード完了後に Tauri の公開鍵で署名と版を検証します。失敗したファイルは適用できず、再ダウンロードできます。更新内容は文字列として表示します。
- 「再起動して更新」を押し、確認すると、未保存の設定を保存してから適用します。分類・ライブラリインポート・BOOTH のダウンロード中は適用を拒否します。画面に表示されたファイル処理中もボタンを無効にします。
- Windows は NSIS の更新モードでインストーラを起動し、現在のプロセスを終了します。インストール後は NSIS が再起動します。Mac は .app を置き換えてからアプリを再起動します。Mac は `/Applications` など書き込み可能な場所へインストールして使ってください。dmg 内からの実行は更新に失敗することがあります。
- ライブラリ DB、設定、ダウンロード先のフォルダは既存のまま使用します。適用に失敗しても準備済みファイルから再試行できます。解決しない場合は「リリース一覧」から手動でインストールできます。
- 拡張 zip はブラウザの拡張管理画面で別途更新します。

## 検証

`npm test` は UI の明示操作、再起動確認の取消、設定保存エラー、再試行、リリース情報生成を検証します。Rust テストは、実際に Tauri CLI で署名した短いテストファイルで、改ざん拒否・版の差し替え拒否・再試行を検証します。テスト鍵は配布には使いません。

Windows / Mac での実インストーラによる更新確認は、公開リポジトリに署名付きの次版を出してから行います。アップデータを持たない v0.1.7 以前からは、対応版を一度手動でインストールしてください。
