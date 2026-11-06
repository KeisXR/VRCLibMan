# リモート同期

複数の PC のあいだで、分類、対応アバター、所持アバター、ファイルとの対応、端末に依存しない設定を受け渡すための仕組みである。アプリに新しい通信路、認証、サーバーは無い。利用者が選んだ場所へ JSON を置くだけである。

現在できるのは**書き出しだけ**である。取り込み、差分、警告、共有フォルダへの自動書き込み、セットアップの移行は入っていない。

## 書き出し

設定の「データ」にある「エクスポート」から、保存ダイアログで選んだ場所へ書き出す。既定の名前は `vrclibman-export-<端末名>-<YYYYMMDD>.json` で、日付は端末の現地日付である。

同じフォルダの一時ファイルに書いてから置き換えるので、途中で失敗しても選んだファイルは壊れない。

## 形式

`format` は `vrclibman-sync`、`version` は `1`。

| キー | 中身 |
| --- | --- |
| `device` | 端末 ID（UUID v4）と端末名 |
| `appVersion` / `exportedAt` | アプリの版と書き出し日時（UTC） |
| `config` | `values`（設定値）と `meta`（キーごとの変更メタ） |
| `products` | `boothId`、`title`、`url`、`classification`、`links` |
| `avatars` | `id`、`displayName`、`baseBody`、`owned`、`aliases`、`notes`、`meta` |
| `fileLinks` | `boothId`、`filename`、`linkSource`、`meta` |
| `counts` | `products`、`links`、`avatars`、`fileLinks`、`config` の実件数 |

`links` は `items`（`avatarId`、`source`）、`locked`、`names`、`unresolvedNames`、`meta` を持つ。`names` と `unresolvedNames` は、受信側の再解決でリンクが消えないために必要である。`source` と `linkSource` は DB の値をそのまま出す。

`meta` は `deviceId`、`deviceName`、`at` で、データのまとまりごとに持つ。履歴が無いときは `null` で、キーは省略しない。`at` は UTC のマイクロ秒固定幅で、文字列のまま並べ替えられる。

配列の順序は固定で、同じ内容なら差分は `exportedAt` だけになる。

## 含めるもの

- 除外していない全商品の ID、商品名、分類、リンク。`url` は DB の値を使わず `https://booth.pm/ja/items/{boothId}` を作る
- 所持アバター、リンク先のアバター、自分で足したアバター、この端末で変更したアバター。触っていない同梱の対応表は出さない
- 商品に結ばれたファイルの名前。同じ商品と名前の行は `manual`、`download`、`auto` の順で 1 件にまとめる
- 次の設定だけ。許可リストは `sync_meta::config_entries` が唯一の定義で、キーが増えても自動では出ない

| 区分 | 設定 |
| --- | --- |
| 分類 | `llmBackend`、`ollamaModel`、`cloudModel`、`confidenceThreshold`、`unloadOllamaAfterClassification` |
| 画面 | `avatarHero`、`boothLinkTarget`、`previewPane`、`browserForceDark`、`checkUpdateOnStartup` |

## 含めないもの

- API キー、Cookie、BOOTH のセッション、拡張のペアリングコード
- ローカルのパス、ライブラリやリモートのフォルダ、Ollama と API の接続先
- 説明文、サムネイル、年齢制限の印、ショップ名
- アバターのカスタムヒーロー画像（ファイルとパス）
- 除外した商品と、それに結ばれたファイル
- フォルダへのリンク、ファイル名として不正な名前
- 画面の幅など、画面や OS に依存する設定
- 共通素体グループそのもの。`baseBody` の id だけを出す

商品名とファイル名は含むので、保存先の管理は利用者が行う。

## 変更メタ

`sync_meta` テーブル（DB版8）に、まとまりごとの指紋と、最後に変えた端末と時刻を置く。指紋はそのまとまりの正規化した値である。

- **値が変わったときだけ記録する。** 起動や、内容の同じ再解決では `at` は進まない。`updated_at` は指紋に入れない
- **移行前の値に時刻を付けない。** v8 への移行、新しい初期データとカタログは `at` が NULL の baseline になる。移行時刻で埋めると、後から更新した端末の自動分類が別の端末の手動分類に勝ってしまう
- **空の状態は勝たせない。** 未分類かつ未ロック未確認の分類と、0 件で未ロックのリンクは、`at` と端末を NULL に戻す
- **書き出しの最初に全件を比べる。** 記録の呼び出しを忘れた変更も、書き出し時刻の `at` で拾う
- 全件比較で初めて見つかった行は baseline になる。スキャンで新しくできた自動リンクは、その時点では `at` が NULL である。利用者が `save_avatar` で作った・直したアバターは、その操作で `at` が付く

端末 ID は初回起動で一度だけ作り、変更しない。端末名の既定は OS のホスト名で、設定の「この端末の名前」から `update_settings` で変える。過去の `sync_meta` に残る端末名は、変更時の名前のままである。

## 未検証

- Windows WebView2 と macOS WKWebView の保存ダイアログから `save_sync_export` までの経路
- Linux の配布版のダイアログと保存
- SMB、NAS、クラウド同期フォルダへの保存。途切れた共有では、保存のスレッドが OS の応答を待ち続ける
- 実ライブラリの規模での全件比較と書き出しの時間

設計の経緯と取り込み以降の予定は、未追跡の `docs/remote-sync-design.md` にある。
