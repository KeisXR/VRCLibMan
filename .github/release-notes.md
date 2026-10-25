VRCLibMan {{APP_VERSION}}（同梱の拡張は {{EXTENSION_VERSION}}）

## ダウンロード

- Windows: `VRCLibMan_{{APP_VERSION}}_x64-setup.exe`（未署名の NSIS インストーラ）
- Mac: `VRCLibMan_{{APP_VERSION}}_aarch64.dmg`（Apple Silicon、macOS 14 以降）
- Chrome / Edge 拡張: `vrclibman-booth-export-{{EXTENSION_VERSION}}.zip`

## v0.1.8 の変更

- 設定の「データ → アプリの更新」から、更新確認・ダウンロード・再起動して適用できるようにしました。操作したときだけ通信します。
- 更新ファイルの署名と版を検証し、改ざんされたファイルを拒否します。進捗・更新内容の表示と、失敗時の再試行に対応しました。
- Windows のテスト起動と、署名テスト用ファイルの改行変換を修正しました。
- 初回導入・更新の手順を追加しました。

初回インストールには上記の exe / dmg を使用します。更新用の `.app.tar.gz`・`.sig`・`latest.json` はアプリ内更新で使用します。

v0.1.7 以前にはアップデータがないため、この版を一度手動でインストールしてください。以後の更新は「アプリの更新」から適用できます。認証なしの更新取得には本体リポジトリの公開が必要です。

## 注意

- Windows 版は未署名です。SmartScreen が「Windows によって PC が保護されました」と出すことがあります。「詳細情報」から「実行」を選んでください。
- Mac 版は ad-hoc 署名だけで、Apple の署名も公証もしていません。Gatekeeper に止められたら、Finder で右クリックして「開く」を選ぶか、システム設定の「プライバシーとセキュリティ」で「このまま開く」を選んでください。それでも開けないときは、`/Applications` に置いたあとで `xattr -dr com.apple.quarantine /Applications/VRCLibMan.app` を実行してください。
- `vrclibman://` は、Mac では .app を `/Applications` に置いたときだけ動きます。
- 拡張は zip を展開し、直下に `manifest.json` があるフォルダを、拡張機能管理画面の「パッケージ化されていない拡張機能を読み込む」で読み込みます。更新するときは同じ場所に上書きして「再読み込み」を押してください。
