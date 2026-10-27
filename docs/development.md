# 開発ガイド

ソースからビルド・テストする人向けの情報です。アプリの導入と操作は [使い方ガイド](../how-to-use.md)、署名鍵とリリース手順は [配布とアプリ内更新](updater.md) を参照してください。変更前に [AGENTS.md](../AGENTS.md) を読んでください。

## ビルドとテスト

Node.js の LTS、Rust（rustup）、各 OS のビルドツールが必要です。Ollama による分類を試す場合は、同じ PC に Ollama とモデルも用意します。

| 環境 | ビルドに必要なもの |
| --- | --- |
| Windows x64 | Visual Studio 2022 Build Tools の C++ ワークロード |
| Apple Silicon Mac | **macOS 27.2 SDK を含む Xcode**、`aarch64-apple-darwin` Rust ターゲット |

Mac のアプリ本体の動作条件は macOS 14 以降ですが、同梱する Apple モデル用ヘルパーをビルドするために SDK 27.2 が必要です。通常の Xcode Command Line Tools だけでは足りません。Release ワークフローは `xcode-27` ランナーで Xcode 27.2 を選びます。

```sh
npm ci
npm run tauri build
```

Mac のターゲットを明示する場合:

```sh
rustup target add aarch64-apple-darwin
npm run tauri build -- --target aarch64-apple-darwin
```

テストとフロントエンドのビルド:

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Rust のコンパイル前に `npm run build` が必要です。Tauri がフロントエンドの成果物を参照します。CI の Mac ライブラリテストは `x86_64-apple-darwin` と Apple モデルを使わないスタブを使用しており、Apple Silicon 配布版の実行確認にはなりません。

## Windows ビルドの高速化

GitHub Actions の Windows x64 ジョブは、Rust ツールチェーンに同梱される LLD (`rust-lld.exe`) をリンクに使用します。
Release の Windows ビルドだけはさらに `codegen-units = 16` と `lto = "thin"` を適用し、`Cargo.toml` の通常の Release プロファイルは変更しません。
これにより macOS のビルド設定、NSIS と Tauri Updater の署名フローは従来どおりです。

Windows のローカルビルドでも LLD を試す場合は PowerShell で次のように指定できます（同梱バイナリがあることを確認してください）。

```powershell
$lld = Join-Path (rustc --print sysroot).Trim() 'lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe'
if (-not (Test-Path -LiteralPath $lld)) { throw "rust-lld.exe not found: $lld" }
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = $lld
npm run tauri build
```

この設定は現在の PowerShell セッションにだけ適用されます。元のリンカーに戻す場合は `Remove-Item Env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER` を実行してください。
異なるリンカーやコンパイル設定は Cargo のキャッシュを一部再構築するため、初回ビルドでは速度差が見えにくい場合があります。
変更前後の比較は同じ条件で複数回実行してください。Windows Release の高速化設定を試すには、Windows で `$env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '16'`、`$env:CARGO_PROFILE_RELEASE_LTO = 'thin'` を追加します。

## 開発実行と画面見本

- `npm run tauri dev` は SQLite やファイル操作を含む実アプリを起動します。インストール版とアプリ識別子が同じため、同じ端末の保存データを使います。
- `npm run dev` はブラウザ用の見本データを表示します。Rust、実モデルの分類、実ファイル操作は動きません。`src/preview/api.ts` の見本実装は、Rust の変更に自動では追従しません。
- JSON 読み込み後の自動分類はデスクトップで初期オン、ブラウザ見本で初期オフです。見本の表示だけで、配布版の初期値や動作を判断しないでください。
- 通常の開発ビルドは、更新用公開鍵を渡さなければアプリ内更新が無効です。設定方法は [配布とアプリ内更新](updater.md) にあります。

## 構成と保存先

フロントエンドは React / TypeScript、デスクトップ部分は Tauri v2 / Rust です。インポート、正規化、ファイル照合、名前解決、分類結果の保存、SQLite を Rust が担当します。

商品・分類・対応表・設定・ブックマークはアプリデータ内の `library.sqlite`、サムネイルは `thumbs` に保存します。

| データ | Windows | macOS |
| --- | --- | --- |
| アプリデータ | `%APPDATA%\app.vrclibman.desktop` | `~/Library/Application Support/app.vrclibman.desktop` |
| メイン WebView | `%LOCALAPPDATA%\app.vrclibman.desktop\EBWebView` | OS の WebKit データストア |

内蔵ブラウザのログイン状態はメイン画面と分離します。Mac は専用の `data_store_identifier` を使います。アンインストールでアプリデータは削除しません。

## 通信と受信口

### 拡張からの商品情報

拡張は開いた商品ページと購入ライブラリを読み、`vrclibman.booth-export` version 1 の商品 JSON をアプリへ渡します。公開商品ページの説明が短い場合は、Cookie を付けない公開 HTML / JSON で補います。HTML が 401 / 403 の場合は JSON へフォールバックしません。

BOOTH へのアクセス間隔は 1000 / 500 / 250ms から選び、未知の値は 1000ms に戻します。ページも説明の補完も並列にしません。429 / 503 は Retry-After を尊重して最大 1 回再試行し、指定待機が 60 秒を超えるときは停止します。250ms の選択時は確認が必要です。

アプリは `127.0.0.1:17321` だけで待ち受け、LAN やインターネットには公開しません。Chrome 拡張形式の Origin と、8 文字のペアリングコードを Bearer token として持つ要求を受け付けます。最初に成功した拡張の Origin を固定し、コードの再発行で固定を解除します。Web ページの Origin はコードが合っていても拒否し、認証失敗が続くと一時的に受信を制限します。

受信する内容は到達確認、商品 JSON、インポート済み商品 ID の照会、自動整理設定の読み書き、ダウンロード完了通知です。完了通知には保存パス・商品 ID・ファイル情報を含みます。拡張はパスワードや Cookie をアプリへ送りません。

### 内蔵ブラウザとファイル取得

内蔵ブラウザはメイン画面とは別の WebView データストアでログイン状態を保持します。商品情報の読み込みには拡張と同じ解析を使い、一般サイトの DOM はインポートしません。

選んだ 1 件のファイルの取得では、WebView から最初の BOOTH ダウンロード URL 用 Cookie を読み、Rust の HTTP クライアントが取得します。通常のダウンロードでブラウザを移動する処理ではありません。Cookie は最初の要求だけに付け、リダイレクト先が BOOTH 内であっても引き継がず、ログへも書きません。ログインが必要な場合はブラウザを開いて案内します。年齢確認は自動通過しません。

自動整理は `Downloads/VRCLibMan/<商品ID>/` 直下の対象ファイルを検査し、指定ライブラリへ移します。別の場所、別の商品 ID、サブフォルダ、`..`、シンボリックリンクは受け付けません。起動時には残ったファイルを 1 回確認し、安全に特定できるものだけを移します。

### 分類と画像

Ollama の接続先は HTTP の `127.0.0.1` / `localhost` / `::1` のみです。Apple のモデルを選んだ場合も、処理を拒否された商品は文字列を伏せて再試行し、導入済み Ollama が利用できればそちらで再試行します。処理の拒否が解消せず、Ollama も利用できない商品は手動確認へ残します。

サムネイルは HTTPS の `pximg.net` / `booth.pm` に制限し、`Referer: https://booth.pm/` を付けます。pximg は `/c/150x150/` に書き換え、4MB まで取得します。

詳細な変更規則とソースの地図は [AGENTS.md](../AGENTS.md)、過去の確認範囲は [検証記録](verification.md) にあります。
