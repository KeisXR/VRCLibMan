# デザインガイド（VRCLibMan 標準デザイン）

VRCLibMan の見た目の決まりを、ほかのプロジェクトへ持ち出せる形でまとめたものです。
変数の写しは同じフォルダの [`tokens.css`](tokens.css) にあります。

- **正本は `src/styles.css`。** このガイドと `tokens.css` は写しで、アプリからは読み込んでいません。`styles.css` の変数や決まりを変えたら、`tokens.css` とこのガイドも合わせてください。
- ここに書いた値は、2026-11-03 時点の main（v0.2.4）の `src/styles.css` から写したものです。推測で補った値はありません。はっきりしない点は「未確認」「要判断」と書いています。

## このデザインの名前と経緯

**今の標準デザイン = 旧「Classic」です。** 新しく作ったものではありません。

| PR | 内容 |
| --- | --- |
| #113 | 設定で選べる「Liquid」を追加。既定は `classic`（設定画面での表示名は「従来」）。同時に、従来デザインに朱色の線のアクセント、サイドバーとメインの背景の分離、アカウントカードを足した |
| #115 | 文字・部品の寸法と角丸を全体的に揃えようとした |
| #116 | #115 の全体的な変更を取り消して元のデザインに戻し、カプセル状の操作部品の角丸だけを `--radius-control` / `--radius-sm` に揃えた |
| #118 | Liquid と設定の「デザイン」セクションを削除し、Classic だけを標準デザインとして残した。旧 DB の `appearance` は読み書きしない |

#118 の前後で、Classic 側の値（色・余白・角丸・影）は変わっていません。#118 で消えたのは `html.theme-liquid` の下にあった Liquid 用の指定と切替の仕組みです。その後に Classic へ入った変更は、選択枠の `--selection-outline`（商品カードと確認画面のタブ）、内蔵ブラウザの自動登録ダイアログ、アセット更新候補の一覧だけです。

このガイドは Classic（標準デザイン）だけを扱います。Liquid の値は載せていません。

## 原則

1. **無彩色が基本。** 主ボタンは黒地に白文字（ダークでは白地に黒文字）。色付きの面はほぼ使いません。
2. **色は線で足す。** アクセントの朱（`--line-accent`）は面を塗らず、選択・見出し・進捗・フォーカスに細い線だけ使います。指定は `styles.css` の「線のアクセント」ブロック1か所にまとめます。
3. **区切りは影と面の差で示す。** カードやパネルに枠線は付けず（`border: 0`）、白い面と薄い影（`--shadow`）で浮かせます。
4. **状態色は文字色。** good / warn / danger は文字の色として使い、背景は塗りません。
5. **OS に合わせる。** フォントは OS の UI フォント、ダークモードは OS 設定、動きは OS の「動きを減らす」に従います。
6. **キーボードで使える。** フォーカス枠は常に 2px で見えるようにします。
7. **曲線に沿ってアクセントを付けない。** 朱の線が角丸の辺に沿って曲がる付け方（角丸の要素への `inset 0 -2px 0` の下線の影、`border-left` などの片側の枠）はしません。許すのは、外周すべてを囲む全周の枠と、曲線に触れない直線だけです。縦棒と下線の両端は丸くてよく（`--accent-cap`、2px まで。要素の角丸には沿わせない独立した線にする）、進捗バーは例外です（[線のアクセントの部品](#線のアクセントの部品)）。

## 色

### ライト / ダーク

| 変数 | ライト | ダーク | 使いどころ |
| --- | --- | --- | --- |
| `--bg` | `#f5f6f8` | `#121214` | ページ背景、右パネル、上部の帯 |
| `--sidebar` | `#eceef1` | `#17171a` | 左サイドバー、ジャンル列 |
| `--divider` | `#e1e3e9` | `#2e2e34` | サイドバーとメインの境目 |
| `--surface` | `#ffffff` | `#1c1c1f` | カード、パネル、選択中のタブ、バナー |
| `--surface-secondary` | `#f7f7f9` | `#26262b` | 入力欄、チップ、バッジ、選択中のトグル |
| `--sunken` | `#f7f7f9` | `#26262b` | 進捗の溝、ログ、アドレス欄 |
| `--text` | `#20222b` | `#f3f3f5` | 本文 |
| `--text-secondary` | `#858995` | `#9b9ba6` | 補足、未選択のナビやタブ |
| `--avatar-hero-sub` | `#5d616c` | `#b4b4be` | アバター詳細ヒーローの補足（kicker / sub） |
| `--muted` | `var(--text-secondary)` | （同じ） | 別名 |
| `--faint` | `#a0a4ae` | `#7e7e88` | 件数ゼロ、小見出し、欠けている状態 |
| `--button` | `#20222b` | `#f5f5f7` | 主ボタンの地、チェックボックスの色 |
| `--button-text` | `#ffffff` | `#161618` | 主ボタンの文字 |
| `--button-hover` | `#000000` | `#ffffff` | 主ボタンのホバー |
| `--accent` | `var(--button)` | （同じ） | 進捗バーなど。色付きではない |
| `--accent-soft` | `var(--hover)` | （同じ） | |
| `--accent-ink` | `var(--button-text)` | （同じ） | |
| `--good` | `#2f6f4e` | `#8fceab` | 成功・手元にある |
| `--warn` | `#8a6a12` | `#e2c36a` | 注意・一部だけ・成人向け表示 |
| `--danger` | `#9d3b32` | `#f0a8a2` | エラー、削除ボタンの文字 |
| `--line` / `--border` | `#e9eaf0` | `#2e2e34` | 区切り線、ポップオーバーの枠 |
| `--hover` | `#f4f5f8` | `#2a2a30` | ホバーの地 |
| `--shadow` | `0 4px 16px rgba(0, 0, 0, 0.06)` | `0 4px 16px rgba(0, 0, 0, 0.28)` | カード・パネル・選択中のタブ |

### 線のアクセント（朱）と選択枠

| 変数 | ライト | ダーク | 使いどころ |
| --- | --- | --- | --- |
| `--line-accent` | `#a6361f` | `#d97757` | ナビの選択棒、ページ見出しと設定見出しの棒、進捗バー、入力欄のフォーカス、タブの下線 |
| `--line-selection` | `rgba(166, 54, 31, 0.55)` | `rgba(217, 119, 87, 0.65)` | 選択中の選択肢（`.choice-row`・`.scope-choices`）の枠 |
| `--selection-outline` | `#7c808b` | `#73737c` | 商品カードと確認画面のタブの 1px 中立色の全周枠 |

| 変数 | 値 | 使いどころ |
| --- | --- | --- |
| `--accent-line` | `2px` | 直線の下線の太さ |
| `--accent-bar` | `3px` | 縦棒の幅（サイドバー・ジャンル・設定の見出し） |
| `--accent-bar-lg` | `4px` | 縦棒の幅（ページ見出し） |
| `--accent-inset` | `var(--radius-sm)` | 直線の下線の左右の余白。角丸の半径以上にして、曲がる部分に触れない |
| `--accent-offset` | `3px` | 直線の下線の、要素の下端からの位置 |
| `--accent-cap` | `2px` | 縦棒と下線の端の丸み（`--accent-line` と同じ値。2px まで） |

#### 線のアクセントの部品

朱（`--line-accent` / `--line-selection`）の指定は、`src/styles.css` の「線のアクセント」ブロック（`==== 線のアクセント（朱）: 指定はこのブロックだけ ====` から `==== 線のアクセントここまで ====`）にだけ書きます。各画面の部品側には書かず、選択中のセレクタをこのブロックの該当する部品の一覧に足します。部品は次の3つだけです。

| 部品 | 書き方 | 今の使いどころ |
| --- | --- | --- |
| 1. 全周の枠 | `border-color`、`box-shadow: 0 0 0 1px`、`outline` で外周すべてを同じ線で囲む。角丸でも全周なら許す | 入力欄のフォーカス、`.rail-icon-button.attention`、選択中の選択肢（`.choice-row`・`.scope-choices`） |
| 2. 直線の縦棒 | `::before` の棒（幅 `--accent-bar`、端の丸み `--accent-cap`） | `.nav-link.active`・`.genre-link.active`（左端、高さ 55%、`left: 2px`）、`.page-head h1`（`--accent-bar-lg`、0.82em）、`.settings-section > header h2`（0.85em）、設定のカテゴリ一覧 `.settings-nav button[aria-current]`（広い幅） |
| 3. 直線の下線 | `::after` の直線。`left` / `right` は `--accent-inset`、`bottom` は `--accent-offset`、高さ `--accent-line`、端の丸み `--accent-cap`（縦棒と同じ 2px）。要素の角丸には沿わない独立した直線 | `.tab.active`、チップ帯 `.chip-bar-item.active`、設定のカテゴリ一覧 `.settings-nav button[aria-current]`（帯のとき） |

- 例外: 進捗バー `.track span` は角丸の溝の中を朱で塗ります（今のまま）。縦棒と下線の端の丸み（2px まで）も曲線に数えません。
- しないこと: 角丸の要素に `inset 0 -2px 0 var(--line-accent)` などの片側だけの影を付ける、`border-left` などの片側の枠を朱にする、縦棒や下線の端を 2px より丸くする、疑似要素でない要素の面を朱で塗る。
- `test/accent-rule.test.mjs` が、ブロックの外での朱の使用、片側の影、片側の枠、下線の角丸を見つけると失敗します。

### 決め打ちの色（変数になっていないもの）

- 内蔵ブラウザの窓の閉じるボタンのホバー: 文字 `#fff`、地 `#c42b1c`（Windows の閉じるボタンに合わせた色と思われるが、意図は未確認）
- 自動登録ダイアログの背景幕: `#0008`
- プレビューの重ね表示の背景幕: `color-mix(in srgb, var(--bg) 78%, transparent)`

## 文字

- フォント: `-apple-system, system-ui, "Segoe UI Variable", "Segoe UI", "Yu Gothic UI", "Hiragino Sans", "Noto Sans JP", "Meiryo", sans-serif`
- 基準: `font-size: 15px`、`line-height: 1.55`
- 太字は `font-weight: 650` を使います（選択中のナビ・タブ、見出し、主ボタン）。例外は設定の「デバッグ」見出しの 700 だけです。
- 数字が並ぶ件数・ズーム率には `font-variant-numeric: tabular-nums` を付けます。

| 用途 | 大きさ | その他 |
| --- | --- | --- |
| ページ見出し `.page-head h1` | 28px | 650、`letter-spacing: -0.03em`。前に朱の棒（幅 4px、高さ 0.82em） |
| 所有一覧の見出し | 20px | `letter-spacing: -0.02em` |
| 節見出し・カード題 | 16px | 650（カード題は `letter-spacing: -0.01em`） |
| 本文・ナビ | 15px | |
| 設定の項目名 | 14px | 650 |
| 補足・メタ情報・フィールド名 | 13px | `--text-secondary` |
| 件数・チップ・バッジ | 12px | |
| 一覧の小見出し `.group h2` | 13px | `--faint`、650、`letter-spacing: 0.02em` |

表示の詰め具合は画面ごとにクラスで変えています。アバター・アセット・ワールド・確認画面は `density-compact`（見出し 26px、カード角丸 `--radius-sm`、画像 48px）、設定は `density-roomy`（内容の余白 48px）です。
- ブランド名は画像ロゴ（Urbanist Black 900 相当）。文字をパスにした `src/assets/brand/vrclibman-wordmark.svg` を文字色のマスクで塗ります。フォントは同梱せず、ほかの文字は OS の UI フォントのままです（作り方は `scripts/make-brand-wordmark.py`）。

## 余白

| 変数 | 値 |
| --- | --- |
| `--space-1` | 4px |
| `--space-2` | 8px |
| `--space-3` | 12px |
| `--space-4` | 16px |
| `--space-5` | 24px |
| `--space-6` | 40px |

- 内容の余白: `var(--space-6) var(--space-6) 64px`。最大幅は `--content-max`（1120px）＋左右の余白。
- サイドバーの幅: `--sidebar-width`（232px）。閉じたときは `--sidebar-collapsed-width`（60px）。
- 幅 860px 以下では内容の余白を `--space-4` にします。サイドバーは上に並べず、列のままアイコンだけにします（[狭い窓](#狭い窓)）。

## 角丸

| 変数 | 値 | 使う部品 |
| --- | --- | --- |
| `--radius-sm` | 10px | タブ、小さな行操作、ナビの行、ジャンルのリンク、ファイル行、ポップオーバー |
| `--radius-control` | 12px | 文字ボタン、検索欄、入力欄、バナー、選択肢の行、メニューの箱 |
| `--radius` | 18px | カード、パネル、空表示、アカウントカード、プレビュー枠 |
| `--radius-lg` | 20px | ファイルのドロップ領域、セットアップカード |
| `--radius-pill` | 9999px | 状態バッジ、チップ、件数、丸いアイコンボタン、閉じる × |

- ツールバーの外側は `calc(var(--radius-control) + 4px)`（内側の入力欄の角丸＋余白 4px）。
- 決まり（`AGENTS.md`）: カプセル状になる文字ボタン・検索欄・ツールバーは `--radius-control`、タブ・小さな行操作は `--radius-sm`、状態バッジと丸いアイコンは `--radius-pill`。文字ボタンを大きなカプセル形にはしません（#115〜#116）。
- 変数を使っていない値もあります: 設定のカード 24px、設定の目次のボタン 10px、内蔵ブラウザの操作部品 6px と 8px、自動登録ダイアログ 12px、サムネイル 8px、在庫表示 `.presence` 999px。揃えるべきかどうかは要判断です。

## 面・影・線

- カード・パネル・空表示: `background: var(--surface)`、`border: 0`、`box-shadow: var(--shadow)`。
- ポップオーバーやメニュー（アカウント、商品の操作、アバター追加）: `--surface` に `1px solid var(--line)` の枠と `--shadow`。
- サイドバー: 地は `--sidebar`、右に `2px solid var(--line)`。メインとの境目に `--divider`。
- 右パネル: 地は `--bg`、左に `1px solid var(--line)`。
- 入力欄: 枠なし、地は `--surface-secondary`。

## 部品

### ボタン

| 種類 | 指定 |
| --- | --- |
| 通常 `.button` | 地なし、文字 `--text`、`padding: 8px 14px`、角丸 `--radius-control`。ホバーで `--hover` |
| 主 `.button.primary` | 地 `--button`、文字 `--button-text`、650。ホバーで `--button-hover` |
| 危険 `.button.danger` | 文字だけ `--danger`。地は塗らない（ホバーは `--hover`） |
| 丸いアイコン `.icon-button` | 36×36px、角丸 `--radius-pill`、地 `--surface`、`--shadow` |
| 無効 | `opacity: 0.45`、`cursor: default` |

### 入力欄

- 検索欄・テキスト・選択欄: 地 `--surface-secondary`、枠なし、角丸 `--radius-control`、`padding: 8px 12px`。
- フォーカス: 検索欄は `box-shadow: inset 0 0 0 1px var(--text)`。フォーム内の入力欄（`.field`・設定の操作列）は `--line-accent` の 1px の線（`box-shadow: 0 0 0 1px var(--line-accent)`）。
- キーボードのフォーカスでは、入力欄に `2px solid var(--line-accent)`、`outline-offset: 2px` を付けます。
- チェックボックスとラジオは `accent-color: var(--button)`。

### カード

- `.card` / `.item` / `.queue-card`: `--surface`、角丸 `--radius`、`--shadow`、枠なし。
- 一覧は `repeat(auto-fill, minmax(260px, 1fr))`、間隔 `--space-3`。
- 選択中の商品カード `.item.selected`: `box-shadow: inset 0 0 0 1px var(--selection-outline)`（1px の中立色の全周枠。朱は使わない）。
- 非公開の商品は画像と本文を `opacity: 0.55`。

### タブ

- `.tab`: 地なし、文字 `--text-secondary`、角丸 `--radius-sm`、`padding: 6px 12px`。ホバーで `--hover`。
- 選択中 `.tab.active`: 地 `--surface`、文字 `--text`、650、影なし。下に朱の直線の下線（`::after`、幅は左右 `--accent-inset` を空けた残り、下端から `--accent-offset`、太さ `--accent-line`、両端は縦棒と同じ 2px の丸み）。角丸の曲がる部分には線を掛けません。
- 確認画面だけは例外で、選択中のタブを `inset 0 0 0 1px var(--selection-outline)` の全周枠にし、朱の下線とページ見出しの棒は出しません。
- 右パネルと内蔵ブラウザのタブ: 選択中は `--surface`＋650＋`--shadow`（朱の線なし）。閉じる × はホバー中・選択中・フォーカス中だけ見せます。

### バッジ・チップ・件数

- `.badge` / `.pill` / `.chip`: 角丸 `--radius-pill`、地 `--surface-secondary`、文字 `--text-secondary`、12px。
- 状態つきのチップ（`.chip.warn` / `.chip.good`）は地を透明にして文字色だけ変えます。成人向けバッジは文字 `--warn`。
- 件数 `.count` は地なし、12px、等幅数字。

### サイドバー・ジャンルナビゲーション

- 行 `.nav-link` / `.genre-link`: 文字 `--text-secondary`、角丸 `--radius-sm`。ホバーで `--hover`。
- 選択中: 地 `--surface`、文字 `--text`、650、`--shadow`、左端に朱の直線の縦棒（幅 `--accent-bar` = 3px、高さ 55%、端の丸み `--accent-cap` = 2px、`left: 2px`）。ボタンの角丸には沿わせません。
- 件数ゼロのジャンルは `--faint`。
- 下部のアカウントカード: 高さ 52px 以上、角丸 `--radius`、`1px solid var(--border)`、`--shadow`、丸いアバター 34px。

#### 左サイドバーの開閉

- 上端は本棚のマーク（`BrandMark`）と画像ロゴです。閉じるとマークと開閉ボタンだけを縦に並べます。
- 各項目に線のアイコン（`src/components/navIcons.tsx`、18px、線幅 1.5、`aria-hidden`）を付けます。絵文字や外部のアイコン集は使いません。
- ブランド名の右の `.rail-toggle`（32px、角丸 `--radius-sm`）で開閉します。`aria-expanded` と `aria-controls` を持ち、名前は「サイドバーを閉じる」/「サイドバーを開く」です。
- 閉じると幅は `--sidebar-collapsed-width`（60px、`src/uiLayout.ts` の `UI_RAIL_COLLAPSED_WIDTH` と同じ値）で固定し、幅の調整ハンドルは出しません。開くと利用者が決めた幅に戻ります。
- 閉じたときはアイコンだけを見せます。項目名と件数は見えない文字として残し、`title` にも「アバター（12）」のように出します。選択中の朱の棒はそのまま出ます。
- 下部は丸いアバター（44px）、再分類と更新の丸いアイコンボタン（36px）だけにし、版と件数の行は隠します。アカウントのメニューは 220px で右へ開きます。
- ジャンルナビゲーションは別の列なので、閉じても今までどおり出ます。サイドバーにジャンルの小分けは入れません。
- 状態は設定 `ui_rail_collapsed`（既定オフ）に保存し、次回の起動でも同じ状態で開きます。
- 幅は動きを付けずに切り替えます。右パネルのネイティブ WebView の矩形が途中でずれないようにするためです（[右パネル](../right-panel.md)）。

#### 狭い窓

列の幅は `src/uiLayout.ts` の `computeAppLayout` が窓幅から決めます（`src/useAppLayout.tsx` がグリッドに入れます）。

| 窓幅 | 左サイドバー | ジャンル |
| --- | --- | --- |
| 861px 以上 | 保存した状態（開く / 閉じる）。開いていて一覧が 480px を切りそうなら、ジャンル列と一緒に最小幅（180px / 160px）まで比例して縮めます | 一覧が 480px 以上残れば列、残らなければチップ帯 |
| 860px 以下 | その場だけアイコンのみ（60px）。`ui_rail_collapsed` には保存しません。開閉ボタンは出しません | 同上 |

- ジャンル列を出すかどうかは、実際の幅で決めます。サイドバー（開閉と保存した幅）、保存したジャンル列の幅（`ui_genre_width`、最小 160px まで縮めて計算）、列になった右パネルの幅、区切り 6px を引いて、一覧に `UI_LIST_MIN_WIDTH`（480px）が残るなら列にします。
  - 右パネルを出していないと、861px 以上では列、860px 以下では 712px 以上で列になります（60 + 6 + 160 + 6 + 480）。
  - 右パネルが列になっている（1000px 以上）ときは、パネルの幅を先に決め、そのうえで残りを判定します。たとえば 1100px でパネル 300px なら、ジャンルはチップ帯になります。
  - 窓幅 1000px 未満の右パネルは重ね表示なので（[右パネル §6.3](../right-panel.md#63-重ね表示狭い窓)）、判定に入れません。
- 窓幅を戻すと、保存した状態のサイドバーに戻ります。
- 右パネルが開いている間は、幅を動かさずに一瞬で切り替えます（`data-rail-motion="fade"`、グリッドに `transition` なし）。右パネルのネイティブ WebView の矩形が途中でずれないようにするためです。右パネルが閉じている間だけ、開閉ボタンを押した直後に幅を `--motion-rail`（140ms）で動かします（`data-rail-motion="slide"`）。窓幅の変化や区切りのドラッグ、起動直後は動かしません。

### チップ帯

ひとつの行に選択肢を並べる帯です。入りきらない分は横にスクロールし、ページを縦にスクロールしても上（`.topbar` の下）に残ります。狭い窓のジャンル（`GenreChipBar`）で使い、ほかの節の切り替え（設定の目次など）にも使えるよう汎用にしています。

| クラス | 役割 |
| --- | --- |
| `.chip-bar` | 外枠の `<nav>`。`position: sticky`、`top: var(--topbar-height, 8px)`、`z-index: 3`、地 `--bg`、下に `0 1px 0 var(--line)` |
| `.chip-bar-track` | 中の行。`display: flex`、`gap: var(--space-1)`、`overflow-x: auto` |
| `.chip-bar-item` | 1つの選択肢（`<a>` か `<button>`）。13px、角丸 `--radius-sm`、地なし、文字 `--text-secondary`。ホバーで `--hover` |
| `.chip-bar-item.active` | 選択中。地 `--surface`、文字 `--text`、650、`box-shadow: var(--shadow)`。下に `.tab.active` と同じ朱の直線の下線（`::after`、[部品3](#線のアクセントの部品)、両端は 2px の丸み）。`aria-current` も付けます |
| `.chip-bar-item.drag-over` | ドラッグ中に重なった選択肢 |
| `.chip-bar-label` / `.chip-bar-count` | 名前と件数。件数は 12px、`--faint`、等幅数字 |
| `.chip-bar-sep` / `.chip-bar-kicker` | まとまりの区切り線（1px × 18px）と小見出し（12px、650、`--faint`） |

- 部品は `src/components/ChipBar.tsx` の `ChipBar` です。`label`（`aria-label`）、`activeKey`（選択が変わったら選択中の項目が見えるよう横にだけスクロール）、`className`（追加のクラス）を受け取り、子に `.chip-bar-item` を並べます。

```tsx
<ChipBar label="設定の目次" activeKey={current} className="settings-chip-bar">
  {sections.map((s) => (
    <a key={s.id} href={`#${s.id}`} className={s.id === current ? "chip-bar-item active" : "chip-bar-item"} aria-current={s.id === current ? "location" : undefined}>
      <span className="chip-bar-label">{s.label}</span>
    </a>
  ))}
</ChipBar>
```

- `--topbar-height` は `App.tsx` が `.topbar` の高さを測って `.main` に入れます。`.main` の外で使うときは `top` を上書きしてください。
- ジャンルのチップ帯は、列と同じリンク・件数・ドロップ先です。「未設定」は件数があるか選択中のときだけ出します。商品をドラッグ中は区切りの後ろにもう一方の種類（アセット / ワールド）のジャンルを出します。

### 設定画面

- 節はカード（角丸 24px、`--surface`、`--shadow`）。見出しの前に朱の棒（幅 3px、高さ 0.85em）。
- 行は「説明（左）｜操作（右寄せ）」の2列で、行の下に `1px solid var(--line)`。狭い幅では1列にします（コンテナクエリ 600px / 800px）。

#### 設定のカテゴリ一覧（`.settings-nav`）

- 部品は `src/components/SettingsNav.tsx`、項目と並びは `src/settingsNav.ts` の `SETTINGS_NAV_ITEMS`。ライブラリ・Unity・起動と常駐・デザイン・更新候補・BOOTH・分類モデル・データ・対応表を編集・デバッグの 10 つで、画面の節の並びと同じです。節の `id`（`settings-unity`、`settings-asset-updates` など）は他の画面からの移動先なので変えません。`settings-app-update` は「データ」の中にあります。
- 「対応表を編集」（`settings-knowledge`）と「デバッグ」（`settings-debug`、閉じた `<details>`）も一覧と同じ列に置きます。一覧から「デバッグ」を押すと開いてから移動します。
- 広い幅: 左の縦の列（168px）で、上に固定（`position: sticky; top: 0`）。今いるカテゴリはサイドバーと同じ示し方（`--surface`＋650＋`--shadow`＋左端の朱の直線の縦棒。[部品2](#線のアクセントの部品)）。
- 設定ページの幅が 800px 以下（コンテナクエリ。窓幅ではないので、左サイドバーの開閉や右パネルの有無に関係なく、設定ページの実幅で決まる）: 上に固定した1行の帯にします。折り返さず横にスクロールし、地は `--bg`、下に `1px` の `--line`。今いるカテゴリは影を消し、`.tab.active` と同じ朱の直線の下線（[部品3](#線のアクセントの部品)、両端は 2px の丸み）にします。縦棒は出しません。帯からはみ出していれば帯の中で見える位置へ寄せます。
- 帯で見出しが隠れないよう、帯の高さ（`--settings-nav-h`、部品が測る）＋12px を `scroll-margin-top` にします。他の画面から `scrollIntoView` で来たときも同じです。
- 今いるカテゴリは `aria-current="location"`。スクロール位置から、見出しが帯の下＋32px か表示域の上 1/4 のどちらか下の線より上にある最後の節にし、末尾まで来たら最後の節にします。押したカテゴリは移動が終わるまで固定します。
- 押すとその節へなめらかに移動し、`prefers-reduced-motion: reduce` ではすぐ移動します。
- 内容が広いときの縦の列は、狭いときの帯と同じ部品です。インポート画面などの別の帯（`.chip-bar` 系）とは今は別の実装で、見た目を揃える余地があります。
- 選択肢の行 `.choice-row`: `1px solid var(--line)`、角丸 `--radius-control`。選択中は枠を `--line-selection`、地を `--surface-secondary`。

## アバター詳細のヒーロー

アバター詳細（`#/avatar/{id}`）の先頭に、所持アバター本体のサムネイルを大きいヒーローとして出せる。

- **スタイル C ハイブリッド:** 画像の幅/高さ ≥ 1.6 なら全幅ヒーロー（上はシャープ、下へ 2 / 8 / 24px の段階ぼかし＋`--bg` への覆い）。未満（正方形・縦長）は、左に鮮明な画像ブロック（角丸 12px）＋同じ画像をぼかして全幅背景に敷く。名前と補足はヒーロー内に直接置く（浮いたカード／角丸ボックスにはしない。左右はコンテンツと同じインセットで隙間なし）。
- **高さ（設定 `avatar_hero`）:** `large` = 400px、`small` = 320px（既定）、`off` = 旧デザイン（画像なしの `.page-head`）。窓幅 860px 以下では大・小とも 220px。
- **基準画像:** 2400×1200（2:1）。横長ケースでは `object-fit: cover` で中央切り抜き。低解像度の注意は横長ヒーローのみ（自然幅がおおよそ 2000px 未満）。
- **コントラスト:** 覆いは `--bg` を 45% → 82% → 100%（上→下）。補足色は `--avatar-hero-sub`（ライト `#5d616c` / ダーク `#b4b4be`）。カード型スクリムは使わない。ライト／ダークとも名前と補足が 4.5:1 以上（`src/avatarHero.ts` と `test/avatar-hero.test.mjs`）。
- **操作:** ヒーロー内にボタンは置かない。「Unity で開く」「BOOTH で開く」は画像の直下の操作行（副ボタン）。新版ダウンロード専用やバージョン選択の入口は作らない。更新を含む取得は、ほかのアセットと同じくダウンロード可能アセット一覧の流れのまま。
- **アセットファイル:** 見出し「アセットファイル」で開閉（件数と合計サイズは常に表示。初期は開。開閉は `localStorage` の `vrclibman.avatarDetail.assetsOpen`）。中身の行は従来のファイル一覧と同じ部品。
- **動き:** スクロールでは `opacity` と `transform` だけ。`prefers-reduced-motion: reduce` では止める。`backdrop-filter` / `mask-image` が使えないときはグラデーションだけに落とす。開閉の chevron も控えめな遷移で、動きを減らすときは止める。
- **配置:** sticky の `.topbar` の直下から、`.content` の左右パディングを打ち消してフルブリード（サイドバー端まで隙間なし）。「アバター一覧へ」はヒーロー内左上。朱の縦棒はヒーローの見出しには付けない。曲線に沿うアクセント線は付けない。
- **設定:** `settings-design`（デザイン）。旧 Liquid / `appearance` の復活ではない。


### ダイアログ・重ね表示

- 内蔵ブラウザの自動登録ダイアログ: `<dialog>` を `showModal()` で開く。`--surface`、`1px solid var(--line)`、角丸 12px、`padding: 24px`、幅 `min(32rem, calc(100vw - 3rem))`、背景幕 `#0008`。
- 「Unity で開く」: プレビューの重ね表示（背景幕は `--bg` の 78%）の上に `.panel`（幅 `min(560px, 100%)`）。

### 通知・エラーの帯

- `.banner`: `--surface`、角丸 `--radius-control`、`--shadow`、`padding: 10px 14px`。出るときに 160ms で 4px 下りながら現れる（`banner-in`）。
- 情報の帯 `.banner.notice`: 文字 `--good`、`role="status"`。一定時間で消えます（基本 6 秒、操作ボタン付きは 12 秒、20 文字を超えると 1 文字 80ms を最大 10 秒まで足す）。マウスが乗っている間、帯の中にフォーカスがある間、画面が隠れている間は止まります（`src/notice.ts`）。
- エラーの帯 `.banner.error`: 文字 `--danger`。時間では消えず、× で閉じるまで残ります。
- 閉じる × `.banner-close`: 28×28px、角丸 `--radius-pill`、地なし、文字 `--text-secondary`、`aria-label="閉じる"`。ホバーで `--hover`。

### 進捗

- 溝 `.track`: 高さ 4px、角丸 `--radius-pill`、地 `--sunken`。中身は `--line-accent`。丸い溝の中を朱で塗るのは、曲線にアクセントを付けない決まりの例外です。
- 終わりが分からないときは幅 40% の帯が 1.1 秒で往復します。
- 部品は `src/ui.tsx` の `<ProgressTrack current total label>` 1つです。`role="progressbar"` で、件数が分かるときは `aria-valuenow`（0〜100）を付けます。同じ溝を画面ごとに書きません。

進捗の送り方と見せ方の決まりです（Rust は `src-tauri/src/progress.rs`、画面は `src/progress.ts`）。

- **件数が分かる仕事は件数のバー、分からない仕事は往復する帯。** `total` に全体の数、`current` に済んだ数を入れます。`total = 0` は「分からない」の意味で、往復する帯になります。分かった時点で件数に切り替えます。
- **終わりの状態は必ず送る。** 始めた仕事は、成功（`done`）、失敗（`error`）、中止（`cancelled`）のどれかで終えます。途中で `?` で抜けても送るように、Rust では `ProgressGuard` を使います（何も送らずに抜けたら `error`「中断しました」）。0 件で終わる仕事も `done` を送ります。終わった仕事は帯に 2.5 秒残してから消えます。`state` を持たない古い形は、`total > 0` で `current >= total` を終わりとみなします。
- **文言は件数を必ず出し、名前は付けてよい。** 件数の分かる仕事は「分類中 3 / 10: 商品名」「コピー中 1 / 1: ファイル名」のように `current / total` を先に書き、今処理している商品名やファイル名を後ろに付けてかまいません（Rust は `progress::counted_message`、名前は 40 文字まで）。長い名前は上部の帯で1行に収め、はみ出す分は省略（…）にします。帯の高さは変えません。検索語、URL、Cookie などは文言にも記録にも入れません。
- **裏で動く仕事（`background: true`）は上部の帯に出さない。** その仕事の設定の欄の中だけで進捗を見せます。アプリ更新の「作業中」にも数えません。
- **上部の帯は1本。** 前面の仕事のうち最後に更新されたものを出し、ほかにも走っている仕事があれば同じ行の右端に「ほか N 件」と出します。行を増やさないので、帯の高さは1件のときと変わりません（右パネルのネイティブ WebView の位置がずれないため。[右パネル](../right-panel.md)）。
- **作業中の判定** は、前面の仕事が1つでも `running` のときだけです。

## 状態のまとめ

| 状態 | 示し方 |
| --- | --- |
| ホバー | 地を `--hover` にし、文字を `--text` に上げる。主ボタンは `--button-hover` |
| キーボードのフォーカス | `:focus-visible` に `2px solid var(--text)`、`outline-offset: 2px`。入力欄は `2px solid var(--line-accent)` |
| 選択中（ナビ・タブ） | `--surface`＋650＋`--shadow`。サイドバー・ジャンルは左の朱の縦棒、`.tab` は下の朱の直線（角丸に掛けない） |
| 選択中（商品カード・確認画面のタブ） | `--selection-outline` の 1px 全周枠だけ |
| 選択中（選択肢） | 枠を `--line-selection`、地を `--surface-secondary` |
| 無効 | `opacity: 0.45`（内蔵ブラウザの操作は 0.4、更新の帯は 0.6）、`cursor: default` |
| ドラッグ中 | 引いている行は `opacity: 0.72`。落とし先は `--hover`＋`inset 0 0 0 1px var(--line)` |

## 動き

- 標準の長さは `--motion`（160ms）、`ease`。地の色・文字色・影・不透明度の変化に使います。
- ページの切替は `rise`（4px 下から現れる）、帯は `banner-in`。
- 小さな要素の出入りは `--motion-fast`（120ms）、カーブは `--ease-out`（`cubic-bezier(0.32, 0.72, 0, 1)`、出だしが速く終わりが柔らかい）。遅らせるときは `--motion-delay`（30ms）。
- 左サイドバーの開閉（[左サイドバーの開閉](#左サイドバーの開閉)）:
  - 幅: `grid-template-columns` を `--motion-rail`（140ms）`--ease-out`。右パネルが閉じているときだけ。
  - 開くとき: 名前・件数・ロゴ・バージョン行が `opacity` と `translateX(-4px → 0)` で 120ms、30ms 遅れて出ます。
  - 閉じるとき: アイコンが `opacity` で 120ms フェードします。
  - 文字は `white-space: nowrap` と `overflow: hidden` で、途中で折り返したり潰れたりしません。
  - 動かすのは `grid-template-columns`・`opacity`・`transform` だけです。幅や高さを持つ他のプロパティは動かしません。
- 押して切り替わる場所は、`styles.css` の「切替の動き」ブロックにまとめています。動かすのは `opacity` と `transform` だけで、幅・高さは動かしません。長さは 120〜160ms です。
  - タブ（`.tab`）・ジャンル列（`.genre-link`）・設定ナビ: 選択中の地・影は `--motion` でフェードします。朱の縦棒・下線は直線のまま、不透明度だけで現れます（`switch-fade`）。
  - タブの中身（確認画面）・ジャンルの一覧（ジャンル列・チップバーで切替）: `switch-rise`（2px 下から現れる）。要素は作り直さず、`useReplayRise` が切替のたびに付け直します。自動で決まった最初のタブでは動かしません。
  - メニュー（`.account-popover`）: フェード＋4px（下に開くメニューは上から）120ms。ダイアログ（「Unityで開く」・取得した商品の分類）: フェード＋4px、`--motion`。背景は 120ms でフェード。
  - 動かさないもの: 右パネルのタブ切替（ネイティブの WebView）。右パネルとそれを含む親要素には `transform` を付けません（WebView の位置がずれるため）。長い一覧の各行にも付けません。
  - 決まりは `test/switch-motion.test.mjs` で確かめます。
- OS の「動きを減らす」（`prefers-reduced-motion: reduce`）では、すべての `animation` と `transition` を止めます。内蔵ブラウザの読み込み中の回転は止めて、半透明の輪にします。

## アクセシビリティ

- キーボードのフォーカス枠は影に頼らず 2px の線で出します（`styles.css` 1564 行）。
- 閉じるボタンなど記号だけのボタンには `aria-label` を付け、記号は `aria-hidden` にします。
- 情報の帯は `role="status"`。読み終わる前に消えないよう、文字数で表示時間を延ばし、マウスやフォーカスがあれば止めます。
- 色だけに頼らない: 選択は面・太字・線を組み合わせ、エラーは時間で消しません。
- 文字のコントラスト比を検証した記録は、今の main にはありません（#115 で 4.5:1 以上を目指した変更は #116 で取り消し）。持ち出す先で確認してください。**未確認**。

## ダークモード

- OS の設定（`prefers-color-scheme: dark`）だけで切り替えます。アプリ内の切替はありません。
- 同じ変数名の値を差し替えるだけで、部品側の指定は変えません。主ボタンは白地に黒文字に反転します。
- 朱のアクセントはダークで明るい `#d97757` に変えます。
- 内蔵ブラウザで表示する外部ページには、別の仕組みとして「強制ダークモード」（設定 `browser_force_dark`、既定オフ）があります。表示用のページにだけ `html { filter: invert(1) }` を入れ、画像・動画・canvas などにもう一度 `invert(1)` を掛けて元の色に戻します。`hue-rotate(180deg)` は色がずれるので使いません。アプリ自身の画面には掛けません（`AGENTS.md`、`src-tauri/src/browser_dark.rs`）。

## やらないこと

- テーマ（Liquid 等）の切替を作らない。標準デザインは1つ（`AGENTS.md`、#118）。アバター詳細のヒーロー高さだけ設定のデザイン節で選ぶ。
- アクセントの朱で面を塗らない。線だけにする。
- 角丸の辺に沿って曲がる朱の線を付けない（片側だけの `inset` の影や `border-left` など）。全周の枠か、曲線に触れない直線にする。
- 状態色で背景を塗らない（`.chip.warn` なども地は透明）。
- カードやパネルに枠線を足さない。影と面の差で分ける。
- 文字ボタン・検索欄・ツールバーを大きなカプセル形にしない（`--radius-control` を使う）。
- 商品カードと確認画面のタブの選択に朱の棒を出さない（中立色の 1px 枠にする）。
- 背景のぼかしや半透明のガラス表現を使わない（Liquid でも使っていなかった）。
- フォーカス枠を消さない。`prefers-reduced-motion` への対応を外さない。

## ほかのプロジェクトで使うには

1. `tokens.css` をコピーして、最初に読み込む。ライト・ダーク・朱のアクセント・フォーカス枠・動きを減らす対応まで入ります（最後の土台の決まりは不要なら消してよい）。
2. 部品は上の「部品」の指定に合わせて、変数だけで書く。色や角丸を直接書かない。
3. 角丸は「文字の操作 = `--radius-control`、タブと小さな行 = `--radius-sm`、バッジと丸アイコン = `--radius-pill`、カード = `--radius`」で選ぶ。
4. 選択の示し方は2通りだけ: ナビゲーション系は「白い面＋太字＋朱の線」、一覧の項目は「`--selection-outline` の 1px 枠」。
5. 持ち出した先で、コントラスト比と実機（Windows / macOS）の見た目を確かめる。

## 分からない点

- #113 のコミットの説明には「3px divider」とありますが、今の `styles.css` でサイドバーの右の線は `2px` です。3px が区切りのつまみの影指定のことかどうかは未確認です。
- 変数を使っていない角丸（24px、10px、8px、6px、999px）と決め打ちの色を、変数に寄せるべきかは決まっていません。
- Windows と macOS の実機での見た目は、このガイドを書くにあたって確認していません。
