# 開発ガイド

ソースからビルド・テストする人向けの情報です。アプリの導入と操作は [使い方ガイド](../how-to-use.md)、署名鍵とリリース手順は [配布とアプリ内更新](updater.md) を参照してください。変更前に [AGENTS.md](../AGENTS.md) を読んでください。

## ビルドとテスト

Node.js の LTS、Rust（rustup）、各 OS のビルドツールが必要です。Ollama による分類を試す場合は、同じ PC に Ollama とモデルも用意します。

| 環境 | ビルドに必要なもの |
| --- | --- |
| Windows x64 | Visual Studio 2022 Build Tools の C++ ワークロード |
| Apple Silicon Mac | **macOS 27.2 SDK を含む Xcode**、`aarch64-apple-darwin` Rust ターゲット |
| Linux x86_64 / aarch64 | Ubuntu 22.04 相当、GTK 3 / WebKitGTK 4.1 開発用パッケージ、CPU に合うネイティブ環境 |

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

## Linux AppImage のビルド

Linux は glibc 系デスクトップを対象に、Ubuntu 22.04 を基準にビルドします。より新しい OS 上でビルドすると、必要な glibc の版も上がる場合があります。x86_64 と aarch64 はそれぞれ同じ CPU の環境で作成します。AppImage のパッケージ化を含むクロスコンパイルは行いません。CI と Release は `ubuntu-22.04` / `ubuntu-22.04-arm` のネイティブランナーを使用します。

Ubuntu のビルド環境には次を導入します。

```sh
sudo apt-get update
sudo apt-get install -y build-essential curl wget file pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils desktop-file-utils dbus
npm ci
npm run tauri build
```

Linux では `src-tauri/tauri.linux.conf.json` を自動で読み込み、AppImage を生成します。成果物は `src-tauri/target/release/bundle/appimage/`（`--target` 指定時はそのターゲット配下）です。更新用の署名設定は [配布とアプリ内更新](updater.md) を参照してください。CI では `APPIMAGE_EXTRACT_AND_RUN=1` を使ってビルドツールを展開実行します。これは配布先で FUSE が不要になる設定ではありません。

Linux のアプリデータは `$XDG_DATA_HOME/app.vrclibman.desktop`（未設定時は `~/.local/share/app.vrclibman.desktop`）です。ダウンロード先は XDG ユーザーディレクトリの設定を読み、保存元の厳密なパス検査は維持します。Linux の常駐は Tauri のトレイと libayatana-appindicator を使います。GIO でセッション D-Bus の `org.kde.StatusNotifierWatcher` と `IsStatusNotifierHostRegistered` を非同期・タイムアウト付きで確認し、表示先がある場合だけ窓なしで常駐します。確認は2秒ごと、1回の上限は2秒です。表示先が失われた場合は窓を再表示し、戻ったときはトレイを再表示します。保存した希望設定は保持します。ログイン時起動は `$XDG_CONFIG_HOME/autostart/VRCLibMan.desktop`（未設定なら `~/.config/autostart`）へ配布 AppImage の外側のパスを登録します。依存先の auto-launch 0.6 が XDG_CONFIG_HOME を無視するため、Linux ではこのファイルだけを直接管理します。パスの特殊文字をエスケープし、一時ファイルから置き換えます。GNOME では AppIndicator 対応の拡張が必要になる場合があります。URL 関連付けの登録に失敗してもアプリを起動でき、起動中のアプリには拡張から送信できます。

Ubuntu / Mint、Fedora、Arch 系の GNOME / KDE、X11 / Wayland は実機確認の対象です。ビルド成功だけではこれらの環境での動作確認済みとはしません。aarch64 もデスクトップ Linux が対象であり、個別の GPU・ボードの動作は別途確認します。

## Linux のネイティブ右パネル確認

Linux の右パネルは、メイン WebView を GtkOverlay の主子にして全面に残し、ページを重ね合わせる子として配置します。別窓では従来の GtkLayout を使います。別窓への移動ではパネル用の余白・配置を戻し、キーとリサイズのハンドラはビューごとに一度だけ登録します。

表示・再配置の変更時には `dbus`、`xdotool`、`xvfb`、`xauth` を用意し、実 Tauri / GTK / WebKit のテストを実行します。既定の Rust テストでは無視され、Linux の CI で100%・200%の両倍率を実行します。合成ページと隔離した保存先を使い、BOOTH のログインや購入履歴は使いません。

```sh
native_root="$(mktemp -d)"
mkdir -p "$native_root"/{data,config,cache,run}
chmod 700 "$native_root/run"
xvfb-run -a -s '-screen 0 3800x2400x24' dbus-run-session -- env \
  VRCLIBMAN_NATIVE_TEST_DIR="$native_root" GDK_BACKEND=x11 GDK_SCALE=1 \
  XDG_DATA_HOME="$native_root/data" XDG_CONFIG_HOME="$native_root/config" \
  XDG_CACHE_HOME="$native_root/cache" XDG_RUNTIME_DIR="$native_root/run" \
  LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
    --features tauri/custom-protocol linux_browser_panel_native -- --ignored --test-threads=1
rm -rf "$native_root"
```

実行前に `npm run build` が必要です。`GDK_SCALE=2` でも別の一時保存先で確認します。名前が `native_root` の一時ディレクトリだけを削除してください。仮想 X11 の成功を Wayland や各デスクトップ・GPU の実機確認とは扱いません。

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

右パネルのページ表示を変更したときは、Linux の `webkit2gtk-driver`、`xvfb`、`xauth` を導入して次も実行します。

```sh
xvfb-run -a -s '-screen 0 2000x1200x24' npm run test:browser-layout
```

見本の API と実際のアプリ画面を使い、WebKit の実レイアウトで表示域の高さ・幅、全面表示、サイズ変更、タブ切替を確認します。DOM の矩形を固定する単体テストでは検出できない高さの縮みを検出します。Windows のネイティブ WebView の埋め込み・分離の実機確認とは別の検証です。CI の Linux ジョブでも実行します。

右パネルのタブ列のクリックやドラッグを変更したときは、Chrome（または Chromium）を入れて次も実行します。`CHROME_PATH` で実行ファイルを指定できます。

```sh
npm run test:tab-click
```

ヘッドレスの Chromium に実際のポインター入力を送り、固定タブとページタブのクリック切替、閾値未満の揺れ、ドラッグ後のクリックを確認します。WebView2 と同じエンジンのクリックの宛先を検証します。

## macOSのネイティブ配置テスト

MacのWKWebViewは、通常のタイトルバーでもDOMの原点と親NSViewの原点が異なります。右パネルの配置を変えたら、次も実行します（RustとSwiftが必要です）。

```sh
native_case="$(mktemp -d)"
rustc test/fixtures/macos-panel-frame.rs -o "$native_case/macos-panel-frame"
swift test/macos-browser-panel-layout.swift "$native_case/macos-panel-frame"
rm -rf "$native_case"
```

本体と同じRustの座標変換を使い、実際のAppKit/WKWebViewにマウス入力を送ります。修正前の配置ではブックマークボタンがページに隠れて押せなくなることと、修正後はブックマークとページの両方を操作できることを確認します。サイズ変更と別ウィンドウへの移動・復帰も対象です。ログインやネットワーク、アプリのDBは使いません。CIの `native macOS browser panel` ジョブでも実行します。

## 構成と保存先

フロントエンドは React / TypeScript、デスクトップ部分は Tauri v2 / Rust です。インポート、正規化、ファイル照合、名前解決、分類結果の保存、SQLite を Rust が担当します。

商品・分類・対応表・設定・ブックマークはアプリデータ内の `library.sqlite`、サムネイルは `thumbs`、選択した商品の大きいプレビューは `previews` に保存します。

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

受信する内容は到達確認、商品 JSON、インポート済み商品 ID と表示中商品の状態の照会、自動整理設定の読み書き、ダウンロード完了通知です。完了通知には保存パス・商品 ID・ファイル情報を含みます。拡張はパスワードや Cookie をアプリへ送りません。

拡張v0.1.16のページ表示・明示的な1件取得・通知の再送は [拡張の仕様](browser-extension.md) を参照してください。`POST /product-status` は同じOrigin固定・Bearer認証の下で100件までのIDだけを受けます。DB移行は不要です。BOOTHのファイル取得をブラウザに任せ、Cookieをアプリへ送らない構成です。

### 内蔵ブラウザとファイル取得

内蔵ブラウザはメイン画面とは別の WebView データストアでログイン状態を保持します。Windows / Linux はアプリのローカルデータディレクトリ配下の `booth-browser` を `data_directory` に指定し、macOS は固定の `BOOTH_DATA_STORE_ID` を `data_store_identifier` に指定します。Cookie の永続化と保存時の保護は OS・WebView の実装に依存し、アプリ独自の暗号化や OS 資格情報ストアへの Cookie の保存は行いません。専用データストアへの分離は、同じ OS ユーザー権限で動く別のプログラムからのアクセスを防ぐ保証ではありません。商品情報の読み込みには拡張と同じ解析を使い、一般サイトの DOM はインポートしません。

利用者向けの推奨は内蔵ブラウザです。実装の初期選択は拡張機能のため、セットアップで内蔵ブラウザを選ぶ手順を案内します。Windows / macOS / Linux では右パネルのページタブと複数の別ウィンドウを併用できます。ページタブは終了時に閉じ、ブックマークは保存します。

取得した商品の自動登録は既定オンです。本人が開いた対応する購入完了ページと、本人がクリックしたダウンロードの保存完了から商品を登録します。ファイルは一意な `Downloads/VRCLibMan-*` へ保存し、商品・バリエーションを一意に特定できたものだけ結び付けます。自動整理オンなら既存のステージング検査を通してライブラリへ移し、オフなら保存先に置いたまま登録します。初回の分類先は利用者が確認し、保留した分類待ちは再起動後も残します。Esc と「後で分類する」は今出ている分だけの後回しで、新しい未分類が届くと確認を開き直します。

選んだ 1 件のファイルの取得では、WebView から最初の BOOTH ダウンロード URL 用 Cookie を読み、Rust の HTTP クライアントが取得します。通常のダウンロードでブラウザを移動する処理ではありません。Cookie は最初の要求だけに付け、リダイレクト先が BOOTH 内であっても引き継がず、ログへも書きません。ログインが必要な場合はブラウザを開いて案内します。年齢確認は自動通過しません。

自動整理は `Downloads/VRCLibMan/<商品ID>/` 直下の対象ファイルを検査し、指定ライブラリへ移します。別の場所、別の商品 ID、サブフォルダ、`..`、シンボリックリンクは受け付けません。起動時には残ったファイルを 1 回確認し、安全に特定できるものだけを移します。

### 分類と画像

Ollama の接続先は HTTP の `127.0.0.1` / `localhost` / `::1` のみです。Apple のモデルを選んだ場合も、処理を拒否された商品は文字列を伏せて再試行し、導入済み Ollama が利用できればそちらで再試行します。処理の拒否が解消せず、Ollama も利用できない商品はカテゴリをその他にして残します。モデルが返した `category` が 8 値以外でも、`ollama::resolve_category` が同義語・ジャンル名・全角などを読んでから決めます。それでも読めないときはバッチを止めず、Ollama があればそちら、無ければ同じくその他（`AppleError::Unreadable`）にします。エラー文とログには受け取った生の `category`（長いときは切り詰め）を残します。Swift ヘルパー側で 8 値に固定する根本対策は別対応です。

「クラウド(APIキー)」を選んだ場合は、BOOTH のカテゴリ・タグだけでは決まらない商品のタイトル・カテゴリ・タグ・説明文（先頭約4,000文字）と分類指示を、指定した OpenAI 互換 API へ送ります。接続先は HTTPS、または同じ PC のループバック HTTP に限ります。キーは OS の資格情報に保存し、DB・ログ・書き出しには含めません。接続テストは `/models` の取得だけで、商品情報は送りません。年齢制限の商品はクラウドへ送らず、利用できる端末内モデルで処理し、なければ未分類に残します。

サムネイルは HTTPS の `pximg.net` / `booth.pm` に制限し、`Referer: https://booth.pm/` を付けます。pximg は `/c/150x150/` に書き換え、4MB まで取得します。

詳細な変更規則とソースの地図は [AGENTS.md](../AGENTS.md)、過去の確認範囲は [検証記録](verification.md) にあります。
