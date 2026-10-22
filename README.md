# VRCLibMan

持っている VRChat / Booth のアバターと、同じ素体で着られる衣装・髪・ギミックを、手元の PC だけで整理するアプリです。

Booth のパスワードやクッキーは持ちません。ブラウザ拡張が、すでにログインしているページの今見えている商品だけを JSON にします。アプリはその JSON を読みます。分類の文章抽出だけを、同じ PC の Ollama に頼みます。どの商品がどのアバターか、どのアバターが同じ素体かは、アプリ内の対応表で決めます。

やらないことは次のとおりです。

- Booth へのログイン、購入、自動ダウンロード
- Unity への転送
- クラウド同期
- 対応表に無い素体関係の推測

## 必要なもの

- Node.js
- Rust（rustup）と Visual Studio 2022 Build Tools の C++ ワークロード
- WebView2（Windows 11 には入っています）
- [Ollama](https://ollama.com/)。分類の初期モデルは `gemma4:12b`（目安 8GB の VRAM）です。`ollama pull gemma4:12b`
- 軽い代替は `ollama pull qwen3:4b`。設定のモデル名で、導入済みのモデルを選べます。すでに保存したモデル名は、更新しても上書きしません。

## インストール（Windows）

公開された GitHub Release から、`VRCLibMan_*_x64-setup.exe` を実行します。これは NSIS のインストーラで、署名は付いていません。SmartScreen が「Windows によって PC が保護されました」と出すことがあります。「詳細情報」から「実行」を選んで進めてください。

現在のユーザーだけに入ります。管理者権限は要りません。実行ファイルは `%LOCALAPPDATA%\VRCLibMan\VRCLibMan.exe` です。WebView2 が無い Windows 10 では、インストーラが小さなブートストラッパをダウンロードして入れます。そのときだけネットが要ります。Windows 11 には WebView2 が入っています。

インストールしたアプリと `npm run tauri dev` は、どちらも識別子 `app.vrclibman.desktop` です。そのためライブラリ `%APPDATA%\app.vrclibman.desktop\library.sqlite`（サムネイルは同じ場所の `thumbs`）と、WebView2 のプロファイル `%LOCALAPPDATA%\app.vrclibman.desktop\EBWebView` を共有します。開発中のウィンドウは、入っているアプリと同じデータベースを開きます。`npm run dev` の見本データはこの場所を使いません。

アンインストールはレジストリの `HKCU\Software\VRCLibMan` を消します。上のライブラリと WebView のフォルダは残します。

同じ Release の `vrclibman-booth-export-*.zip` が Chrome 拡張です。展開すると直下に `manifest.json` があります。`chrome://extensions` の「パッケージ化されていない拡張機能を読み込む」で、そのフォルダを指定します。

手元でインストーラを作る手順は、下の「開発」にあります。

macOS 用のインストーラはまだ作りません。

## 起動

開発中は次でウィンドウを開きます。

```powershell
npm install
npm run tauri dev
```

`npm run dev` でポート 1420 を開いたまま `npm run tauri dev` を実行すると、Vite が二重に起動して失敗します。先に `npm run dev` を止めてから実行してください。`npm run tauri dev` はインストール版と同じ識別子なので、上に書いた `%APPDATA%` のデータベースと WebView プロファイルを開きます。

画面だけの確認は `npm run dev` で http://localhost:1420 を開きます。このとき出るのは見本データで、抽出・フォルダ調査・Ollama は動きません。

## Booth から書き出す

1. Chrome か Edge で `extension` フォルダを「パッケージ化されていない拡張機能」として読み込む
2. Booth に自分でログインし、ライブラリか商品ページを開く
3. 拡張の「このページを書き出す」を押す。ライブラリ全体は、ライブラリを開いて「ライブラリをすべて書き出す」
4. VRCLibMan の「取り込み」へ JSON をドロップする
5. 「未抽出を分類」で Ollama にカテゴリと、文中のアバター名だけを抜かせる
6. アバターを開くと、その素体と共通素体の衣装が見える

説明文の補完は、拡張が商品の公開ページを `credentials: "omit"` で1件ずつ取ります。年齢確認で本文が落ちるときは、同じ条件で商品の JSON を読みます。ログイン中のセッションは、説明の補完には送りません。ライブラリ全ページの読み取りだけ、すでにブラウザで開いているログインを使います。

Ollama の接続先は `http://127.0.0.1` か `localhost` だけです。温度は 0.1 固定です。初期モデルは `gemma4:12b` で、目安として 8GB の VRAM を使います。`qwen3:4b` は軽い代替で、設定から導入済みのモデルを選べます。分類で何をアバター名にし、何を確認に残すかは `AGENTS.md` の「抽出と確認の契約」に書いてあります。

## 初期データ

対応表の初期値は、作者の商品説明で衣装の着せ替えができると確認できた「あまとうさぎ共通素体」（ライム、プラム、シフォン、ショコラ）だけです。マヌカ、しなの、セレスティア、ミルティナ、桔梗、ルルネは別名だけで、同じ素体には入れていません。関係を足すときは設定の対応表で、根拠が分かるメモと一緒に追加してください。削除した初期データは、起動のたびに戻りません。「初期データを追加」で足りない行だけ戻ります。

フォルダ調査は、ドライブ直下、Windows、Program Files を拒否します。深さ 8、2 万件までです。

## 開発

```powershell
npm test
cargo test --manifest-path src-tauri/Cargo.toml
```

`npm test` は拡張のページ解析です。`cargo test` は取り込み、名前の解決、ファイル名の突合、画面用の集計です。

Windows でインストーラを手元で作るときは、上の「必要なもの」に加えて NSIS を Tauri が初回に取ります。

```powershell
npm install
npm run tauri build
```

できたファイルは `src-tauri\target\release\bundle\nsis\VRCLibMan_0.1.0_x64-setup.exe` です。中の実行ファイルは `VRCLibMan.exe` です。ARM の Windows では `x64` が `arm64` になります。版は `package.json`、`src-tauri\tauri.conf.json`、`src-tauri\Cargo.toml` の `version` を同じにしてください。GitHub へ載せるときは、その版のタグ `v0.1.0` を push します。workflow が下書き Release に、未署名の setup.exe と拡張の zip を置きます。コード署名の秘密情報は入れていません。
