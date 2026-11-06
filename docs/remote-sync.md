# データ移行

PC のあいだで、商品情報、サムネイル、ヒーロー画像、分類、対応アバター、所持アバター、ファイルとの対応、端末に依存しない設定を受け渡すための仕組みである。受け取る PC が商品情報と画像を取り直さなくて済むように、画像はファイルのまま含める。PC どうしは対等で、親機は無い。アプリに新しい通信路、認証、サーバーは無い。利用者が選んだ場所へ ZIP を置くだけである。

現在できるのは**書き出しだけ**である。取り込み、差分、上書きの警告と取り消し、共有フォルダへの自動書き込み、セットアップの移行は入っていない。

設定の「リモートへ自動同期」（`remote_sync.rs`）は、手元のファイルをリモートライブラリへコピーする別の機能で、この文書の対象ではない。

## 書き出し

設定の「データ移行」の「エクスポート」から、保存ダイアログで選んだ場所へ書き出す。既定の名前は `vrclibman-migration-<端末名>-<YYYYMMDD>.zip` で、日付は端末の現地日付である。

同じフォルダの一時ファイルに書いてから置き換えるので、途中で失敗しても選んだファイルは壊れない。データベースのロックは行を読むあいだだけ持ち、画像の読み込みと ZIP の書き込みはロックの外で行う。

`npm run dev` の見本は ZIP を作らず、画像の参照がすべて `null` の manifest だけを `…-manifest.json` として保存する。

## 形式

ZIP の中身は次のとおり。

| エントリ | 中身 |
| --- | --- |
| `manifest.json` | 下の表の JSON。Deflate で圧縮する |
| `files/thumbnails/<boothId>.<拡張子>` | キャッシュ済みのサムネイル。圧縮しない |
| `files/heroes/<avatarId>.<拡張子>` | 利用者が指定したヒーロー画像。圧縮しない |

拡張子はキャッシュしたファイルのものを小文字にして使う。ID がそのままファイル名にできないアバターは、`hero_image::file_stem` の安全な名前（ハッシュ付き）になる。読む側は ID から名前を組み立てず、`thumbnail` と `heroImage` の値でエントリを探す。

`manifest.json` は ZIP の最後に書く。実際に書けた画像だけを載せるためである。読む側はエントリの順序に頼らず、名前で探す。

`format` は `vrclibman-sync`、`version` は `2`。

| キー | 中身 |
| --- | --- |
| `device` | 端末 ID（UUID v4）と端末名 |
| `appVersion` / `exportedAt` | アプリの版と書き出し日時（UTC） |
| `config` | `values`（設定値）と `meta`（キーごとの変更メタ） |
| `products` | `boothId`、`title`、`shop`、`descriptionSnippet`、`url`、`thumbnail`、`classification`、`links` |
| `avatars` | `id`、`displayName`、`baseBody`、`owned`、`aliases`、`notes`、`heroImage`、`meta` |
| `fileLinks` | `boothId`、`filename`、`linkSource`、`meta` |
| `counts` | `products`、`links`、`avatars`、`fileLinks`、`config`、`thumbnails`、`heroImages` の実件数 |

`thumbnail` と `heroImage` は ZIP の中の相対パスで、画像を入れなかったときは `null` である。`counts` の `thumbnails` と `heroImages` は、実際に書いたエントリの数である。

`links` は `items`（`avatarId`、`source`）、`locked`、`names`、`unresolvedNames`、`meta` を持つ。`names` と `unresolvedNames` は、受信側の再解決でリンクが消えないために必要である。`source` と `linkSource` は DB の値をそのまま出す。

`meta` は `deviceId`、`deviceName`、`at` で、データのまとまりごとに持つ。履歴が無いときは `null` で、キーは省略しない。`at` は UTC のマイクロ秒固定幅で、文字列のまま並べ替えられる。

配列の順序は固定で、同じ内容なら `manifest.json` の差分は `exportedAt` だけになる。

### 版の違い

| 版 | 形 | 画像と商品情報 |
| --- | --- | --- |
| 1 | JSON 単体 | 商品は ID と商品名だけ。画像は無い |
| 2 | ZIP（`manifest.json` と `files/`） | ショップ名、説明文、サムネイル、ヒーロー画像を含む |

版 2 は版 1 のキーを残したまま足している。アプリに取り込みはまだ無いので、読み手の移行は要らない。取り込みを作るときは、`version` を見て版 1 の JSON も読めるようにする。

## 含めるもの

- 除外していない全商品の ID、商品名、ショップ名、説明文、分類、リンク。`url` は DB の値を使わず `https://booth.pm/ja/items/{boothId}` を作る
- それらの商品のキャッシュ済みサムネイルのファイル。ファイルが無い、または読めない商品は `thumbnail` を `null` にし、書き出しは止めない
- 利用者が指定したヒーロー画像のファイル。読めないときも同じく `heroImage` を `null` にする
- 所持アバター、リンク先のアバター、自分で足したアバター、この端末で変更したアバター、ヒーロー画像を指定したアバター。触っていない同梱の対応表は出さない
- 商品に結ばれたファイルの名前。同じ商品と名前の行は `manual`、`download`、`auto` の順で 1 件にまとめる
- 次の設定だけ。許可リストは `sync_meta::config_entries` が唯一の定義で、キーが増えても自動では出ない

| 区分 | 設定 |
| --- | --- |
| 分類 | `llmBackend`、`ollamaModel`、`cloudModel`、`confidenceThreshold`、`unloadOllamaAfterClassification` |
| 画面 | `avatarHero`、`boothLinkTarget`、`previewPane`、`browserForceDark`、`checkUpdateOnStartup` |

## 含めないもの

- API キー、Cookie、BOOTH のセッション、拡張のペアリングコード
- ローカルのパス、ライブラリやリモートのフォルダ、Ollama と API の接続先。画像の場所は ZIP の中の相対パスだけを書く
- サムネイルの URL、年齢制限の印
- 除外した商品と、それに結ばれたファイルと画像
- フォルダへのリンク、ファイル名として不正な名前
- 画面の幅など、画面や OS に依存する設定
- 共通素体グループそのもの。`baseBody` の id だけを出す

商品名、ショップ名、説明文、画像、ファイル名は含むので、保存先の管理は利用者が行う。

年齢制限の商品も、除外していなければ版 1 と同じく対象になる。DB にある説明文とサムネイルが入り、年齢制限の印は入らない。取り込みを作るときは、受け取る PC で R-18 の表示とクラウド分類からの除外をどう保つかを先に決める。

## 変更メタ

`sync_meta` テーブル（DB版8）に、まとまりごとの指紋と、最後に変えた端末と時刻を置く。指紋はそのまとまりの正規化した値である。ショップ名、説明文、サムネイル、ヒーロー画像は指紋に入らないので、これらだけの変更では `at` は進まない。

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
- 実ライブラリの規模での全件比較と書き出しの時間、ZIP の大きさ

設計の経緯と取り込み以降の予定は、未追跡の `docs/remote-sync-design.md` にある。
