VRCLibMan {{APP_VERSION}}（同梱の拡張は {{EXTENSION_VERSION}}）

## ダウンロード

- Windows: `VRCLibMan_{{APP_VERSION}}_x64-setup.exe`（未署名の NSIS インストーラ）
- Mac: `VRCLibMan_{{APP_VERSION}}_aarch64.dmg`（Apple Silicon、macOS 14 以降）
- Linux x86_64（実機未検証のテスト版）: `VRCLibMan_{{APP_VERSION}}_amd64.AppImage`
- Linux ARM64（実機未検証のテスト版）: `VRCLibMan_{{APP_VERSION}}_aarch64.AppImage`
- Chrome / Edge 拡張: `vrclibman-booth-export-{{EXTENSION_VERSION}}.zip`

## v{{APP_VERSION}} の変更

{{CHANGES}}

## 再分類のおすすめ

{{RECLASSIFY_NOTE}}

初回導入には上記の exe / dmg / AppImage を使用します。AppImage は実行権限を付け、ユーザー自身がファイルを置き換えられる場所（例: `~/Applications`）へ保存してください。AppImage はアプリ内更新にも使います。更新用の `.app.tar.gz`・`.sig`・`latest.json` は直接開く必要はありません。

アプリ内更新を利用できない版からは、上記のインストーラで手動更新してください。ライブラリと設定は引き継がれます。

## 注意

- Windows 版は未署名です。SmartScreen が「Windows によって PC が保護されました」と出すことがあります。「詳細情報」から「実行」を選んでください。
- Mac 版は ad-hoc 署名だけで、Apple の署名も公証もしていません。Gatekeeper に止められたら、Finder で右クリックして「開く」を選ぶか、システム設定の「プライバシーとセキュリティ」で「このまま開く」を選んでください。それでも開けないときは、`/Applications` に置いたあとで `xattr -dr com.apple.quarantine /Applications/VRCLibMan.app` を実行してください。
- `vrclibman://` は、Mac では .app を `/Applications` に置いたときだけ動きます。
- Linux 版は実機未検証のテスト版です。各リリースで両 CPU の AppImage を CI でビルド・署名していますが、配布先での起動・各機能・アプリ内更新の動作は未検証です。Ubuntu 22.04 をビルドの基準とし、glibc 2.35 以降と AppImage の実行に対応する FUSE 環境を対象にしています。
- 拡張は zip を展開し、直下に `manifest.json` があるフォルダを、拡張機能管理画面の「パッケージ化されていない拡張機能を読み込む」で読み込みます。更新するときは同じ場所に上書きして「再読み込み」を押してください。
