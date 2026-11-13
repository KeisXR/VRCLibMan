import { ASSET_GENRES, GENRE_FREE, GENRE_LABEL, GENRE_UNSET, WORLD_GENRES } from "./types";
import { genreHref } from "./genreBrowseUtils";
import { SETTINGS_NAV_ITEMS } from "./settingsNav";

/**
 * 検索ページの「画面と設定」。サイドバーの項目、各画面の中の項目、ジャンル、設定の節と行を、
 * 移動先と一緒に手で並べた索引。設定画面は開くまで DOM が無いので、画面から読み取らずにここへ書く。
 * `target` の id は `test/search.test.mjs` がソースに実在するか確かめる。詳しくは docs/design/README.md の「検索」。
 */

/** その項目が画面に出る条件。満たさないときも結果には出し、注記を付けて `fallback` へ移る。 */
export type AppSearchRequirement = "builtinBrowser" | "extension";

export interface AppSearchEntry {
  /** 一意な鍵。 */
  key: string;
  title: string;
  /** どこにあるか。サイドバーの項目名から順に。 */
  path: readonly string[];
  /** 検索だけに使う別の言い方。説明文を丸ごと入れると、ありふれた語でも当たるので入れない。 */
  keywords?: readonly string[];
  href: string;
  /** 移ったあとに見せる要素の id。無ければページの先頭。 */
  target?: string;
  /** `target` が出ていないとき（条件を満たさない・まだ無い）に見せる親の id。 */
  fallback?: string;
  requires?: AppSearchRequirement;
}

/** 今の状態。`extension` は読み込み方が分からないとき null（注記を出さない）。 */
export interface AppSearchContext {
  builtinBrowser: boolean;
  extension: boolean | null;
}

const SIDEBAR = "サイドバー";
const IMPORT = "インポート";
const PURCHASES = "購入履歴";
const LOG = "ログ";
const SETTINGS = "設定";

/** 設定の節の見出しと別の言い方。並びと id は `SETTINGS_NAV_ITEMS`。 */
const SETTINGS_SECTIONS: Record<string, { title: string; keywords: readonly string[] }> = {
  "settings-library": { title: "ライブラリとダウンロード", keywords: ["保存先", "自動整理", "リモート"] },
  "settings-unity": { title: "Unity", keywords: ["unitypackage", "Unityで開く"] },
  "settings-startup": { title: "起動と常駐", keywords: ["バックグラウンド", "常駐", "トレイ", "自動起動"] },
  "settings-design": { title: "デザイン", keywords: ["見た目", "ヒーロー"] },
  "settings-asset-updates": { title: "アセットの更新設定", keywords: ["更新確認", "アセットの更新"] },
  "settings-booth": { title: "BOOTH の読み込み方", keywords: ["拡張機能", "内蔵ブラウザ", "両方", "読み込み方", "ペアリング"] },
  "settings-data": { title: "バージョン情報", keywords: ["バージョン", "アップデート", "ライセンス", "データ移行"] },
  "settings-knowledge": { title: "対応表を編集", keywords: ["対応表", "別名", "素体", "共通素体", "アバターを追加"] },
  "settings-debug": { title: "デバッグ", keywords: ["メンテナンス", "分類改善"] },
};

function sectionTitle(id: string): string {
  return SETTINGS_SECTIONS[id]?.title ?? id;
}

function setting(
  section: string,
  key: string,
  title: string,
  keywords: readonly string[],
  extra: Partial<Pick<AppSearchEntry, "target" | "fallback" | "requires">> = {},
): AppSearchEntry {
  return {
    key: `setting:${key}`,
    title,
    path: [SETTINGS, sectionTitle(section)],
    keywords,
    href: "#/settings",
    target: extra.target ?? key,
    fallback: extra.fallback ?? section,
    ...(extra.requires ? { requires: extra.requires } : {}),
  };
}

const NAV_ENTRIES: AppSearchEntry[] = [
  { key: "nav:avatars", title: "アバター", path: [SIDEBAR], keywords: ["所持アバター", "アバター一覧"], href: "#/" },
  { key: "nav:assets", title: "アセット", path: [SIDEBAR], keywords: ["アセット一覧", "ジャンル"], href: "#/assets" },
  { key: "nav:worlds", title: "ワールド", path: [SIDEBAR], keywords: ["ワールド一覧", "ジャンル"], href: "#/worlds" },
  { key: "nav:purchases", title: PURCHASES, path: [SIDEBAR], keywords: ["注文", "買った"], href: "#/purchases" },
  { key: "nav:log", title: LOG, path: [SIDEBAR], keywords: ["記録", "分類記録", "アセット更新記録"], href: "#/classification-log" },
  { key: "nav:import", title: IMPORT, path: [SIDEBAR], keywords: ["読み込み", "取り込み"], href: "#/import" },
  { key: "nav:settings", title: SETTINGS, path: [SIDEBAR], keywords: ["環境設定", "オプション"], href: "#/settings" },
];

const PAGE_ENTRIES: AppSearchEntry[] = [
  { key: "import:booth", title: "1. BOOTH から読み込む", path: [IMPORT], keywords: ["BOOTH", "読み込み", "取り込み", "受信"], href: "#/import", target: "import-booth" },
  { key: "import:exclusions", title: "商品を管理・除外", path: [IMPORT, "1. BOOTH から読み込む"], keywords: ["除外", "除外を解除", "非表示"], href: "#/import", target: "import-exclusions", fallback: "import-booth" },
  { key: "import:extension", title: "拡張機能の使い方・受信履歴", path: [IMPORT, "1. BOOTH から読み込む"], keywords: ["拡張機能", "Chrome", "Edge", "受信履歴"], href: "#/import", target: "import-extension-help", fallback: "import-booth", requires: "extension" },
  { key: "import:browser", title: "内蔵ブラウザを開く", path: [IMPORT, "1. BOOTH から読み込む"], keywords: ["ブラウザを開く", "ライブラリを開く", "ログイン"], href: "#/import", target: "import-builtin-browser", fallback: "import-booth", requires: "builtinBrowser" },
  { key: "import:walk", title: "ライブラリ全取得", path: [IMPORT, "1. BOOTH から読み込む"], keywords: ["全取得", "商品情報を取得", "ギフト", "無料ダウンロード"], href: "#/import", target: "import-library-walk", fallback: "import-booth", requires: "builtinBrowser" },
  { key: "import:classification", title: "2. 分類", path: [IMPORT], keywords: ["未分類を分類", "古い分類", "再分類", "中断"], href: "#/import", target: "import-classification" },
  { key: "import:reclassify", title: "すべて再分類", path: [IMPORT, "2. 分類"], keywords: ["再分類", "分類し直す"], href: "#/import", target: "import-reclassify-all", fallback: "import-classification" },
  { key: "import:files", title: "3. 手元のファイル", path: [IMPORT], keywords: ["フォルダを調べる", "スキャン", "照合", "商品未設定のファイル"], href: "#/import", target: "import-existing-files" },
  { key: "import:existing", title: "ダウンロード済みファイルを登録", path: [IMPORT, "3. 手元のファイル"], keywords: ["取り込み元を選ぶ", "Downloads", "zip", "unitypackage"], href: "#/import", target: "import-register-existing", fallback: "import-existing-files" },
  { key: "import:json", title: "JSON ファイルから読み込む", path: [IMPORT, "その他"], keywords: ["JSON", "書き出したファイル"], href: "#/import", target: "import-json", fallback: "import-other" },
  { key: "import:free", title: "ライブラリにない無料商品を追加", path: [IMPORT, "その他"], keywords: ["無料商品", "URL"], href: "#/import", target: "import-free-product", fallback: "import-other" },

  { key: "purchases:history", title: "履歴", path: [PURCHASES], keywords: ["注文一覧", "購入履歴"], href: "#/purchases" },
  { key: "purchases:summary", title: "アバター別", path: [PURCHASES], keywords: ["集計", "アバターごと", "購入履歴"], href: "#/purchases/summary" },
  { key: "purchases:read", title: "購入履歴を読み込む", path: [PURCHASES], keywords: ["注文の詳細"], href: "#/purchases", target: "purchase-read", requires: "builtinBrowser" },
  { key: "purchases:prices", title: "金額を表示", path: [PURCHASES], keywords: ["価格", "代金"], href: "#/purchases", target: "purchase-show-prices" },
  { key: "purchases:search", title: "購入履歴の検索と絞り込み", path: [PURCHASES, "履歴"], keywords: ["注文番号", "ラベル", "年"], href: "#/purchases", target: "purchase-search" },
  { key: "purchases:labels", title: "ラベルを管理", path: [PURCHASES, "履歴"], keywords: ["ラベル", "ギフト"], href: "#/purchases", target: "purchase-labels" },

  { key: "log:classification", title: "分類記録", path: [LOG], keywords: ["分類の履歴", "元に戻す", "Undo"], href: "#/classification-log" },
  { key: "log:unresolved", title: "未収録の名前", path: [LOG, "分類記録"], keywords: ["対応表に無い名前", "Issue"], href: "#/classification-log", target: "classification-unresolved" },
  { key: "log:asset-update", title: "アセット更新", path: [LOG], keywords: ["アセット更新記録", "更新の履歴"], href: "#/asset-update-log" },
  { key: "log:asset-pending", title: "未確認の更新", path: [LOG, "アセット更新"], keywords: ["確認済みにする", "更新あり"], href: "#/asset-update-log", target: "asset-update-pending", fallback: "asset-update-history" },
  { key: "log:asset-history", title: "検出履歴", path: [LOG, "アセット更新"], keywords: ["更新の履歴"], href: "#/asset-update-log", target: "asset-update-history" },
];

function genreEntries(kind: "asset" | "world"): AppSearchEntry[] {
  const base = kind === "asset" ? "#/assets" : "#/worlds";
  const page = kind === "asset" ? "アセット" : "ワールド";
  const genres: readonly string[] = kind === "asset" ? ASSET_GENRES : WORLD_GENRES;
  return [...genres, GENRE_FREE, GENRE_UNSET].map((genre) => ({
    key: `genre:${kind}:${genre}`,
    title: GENRE_LABEL[genre] ?? genre,
    path: [page, "ジャンル"],
    href: genreHref(base, genre),
  }));
}

const SETTING_ENTRIES: AppSearchEntry[] = [
  ...SETTINGS_NAV_ITEMS.map(({ id, label }) => ({
    key: `setting-section:${id}`,
    title: sectionTitle(id),
    path: [SETTINGS],
    keywords: [label, ...(SETTINGS_SECTIONS[id]?.keywords ?? [])],
    href: "#/settings",
    target: id,
  })),
  setting("settings-library", "settings-library-folder", "ライブラリのフォルダ", ["保存先", "移動先", "スキャン"]),
  setting("settings-library", "settings-remote-library", "リモートライブラリ", ["ネットワークドライブ", "NAS", "共有フォルダ"]),
  setting("settings-library", "settings-remote-auto-sync", "リモートへ自動同期", ["同期", "自動コピー", "Nextcloud"]),
  setting("settings-library", "settings-remote-about", "リモートライブラリについて", ["同期", "競合"]),
  setting("settings-library", "settings-route-downloads", "ダウンロードを自動整理", ["自動整理", "振り分け", "ダウンロードフォルダ"]),
  setting("settings-library", "settings-route-about", "自動整理について詳しく", ["自動整理"]),
  setting("settings-unity", "settings-unity-editor", "既定の Unity", ["Unity のバージョン", "エディタ", "Unityで開く"]),
  setting("settings-unity", "settings-unity-detect", "Unity を探す", ["Unity Hub", "参照", "検出"]),
  setting("settings-startup", "settings-background", "バックグラウンドで動作", ["常駐", "トレイ", "メニューバー", "閉じても動かす"]),
  setting("settings-startup", "settings-launch-at-login", "ログイン時に起動", ["自動起動", "スタートアップ"]),
  setting("settings-design", "settings-avatar-hero", "アバター詳細のヒーロー", ["ヒーロー画像", "見出し", "旧デザイン"]),
  setting("settings-asset-updates", "settings-asset-updates-all", "全商品をまとめて確認対象にする", ["更新確認", "確認対象"]),
  setting("settings-asset-updates", "settings-asset-updates-auto", "1日1回の自動確認", ["自動確認", "更新確認"]),
  setting("settings-asset-updates", "settings-asset-updates-add", "確認対象に追加", ["更新確認", "商品を選択"]),
  setting("settings-asset-updates", "settings-asset-updates-check", "今すぐ確認", ["更新確認", "アセットの更新"]),
  setting("settings-booth", "settings-extension-pairing", "ペアリングコード", ["拡張機能をつなぐ", "コードをコピー", "コードを作り直す"], { requires: "extension" }),
  setting("settings-booth", "settings-booth-interval", "BOOTH へのアクセス間隔", ["間隔", "ライブラリ全取得", "速さ"], { requires: "builtinBrowser" }),
  setting("settings-booth", "settings-browser-troubleshooting", "内蔵ブラウザのトラブルシューティング", ["診断情報", "ログ"], { requires: "builtinBrowser" }),
  setting("settings-booth", "settings-booth-link-target", "商品ページを開くブラウザ", ["リンク", "既定のブラウザ", "内蔵ブラウザ"], { requires: "builtinBrowser" }),
  setting("settings-booth", "settings-force-dark", "強制ダークモード", ["ダーク", "ダークテーマ", "暗い", "反転"], { requires: "builtinBrowser" }),
  setting("settings-booth", "settings-force-dark-about", "強制ダークモードについて", ["ダーク"], { requires: "builtinBrowser" }),
  setting("settings-booth", "settings-auto-register", "取得したアセットの自動登録", ["自動登録", "購入完了", "ダウンロード完了"], { requires: "builtinBrowser" }),
  setting("settings-booth", "settings-clear-browser-data", "内蔵ブラウザの閲覧データを削除", ["ログアウト", "Cookie", "閲覧データ"], { requires: "builtinBrowser" }),
  setting("settings-data", "settings-version", "バージョン", ["版", "拡張機能のバージョン"]),
  setting("settings-data", "settings-license", "ライセンス", ["ソースコード", "MPL", "第三者のライセンス", "GitHub"]),
  setting("settings-data", "settings-check-update-on-startup", "起動時にアップデートを確認", ["アップデート", "更新"]),
  setting("settings-data", "settings-app-update", "アプリの更新", ["アップデート", "更新を確認", "再起動して更新"]),
  setting("settings-data", "settings-device-name", "この端末の名前", ["端末名", "PC の名前"]),
  setting("settings-data", "settings-data-export", "データ移行", ["書き出し", "エクスポート", "ZIP", "別の PC", "バックアップ"]),
  setting("settings-data", "settings-catalog", "同梱の対応表", ["対応表を更新", "出典", "利用規約"]),
  setting("settings-data", "settings-privacy", "プライバシー", ["送信", "個人情報"]),
  setting("settings-debug", "settings-classification-feedback", "分類改善用データ", ["JSON", "書き出し", "その他", "未分類"]),
  setting("settings-debug", "settings-maintenance", "メンテナンス", ["初期データを追加", "対応表を再適用", "セットアップを再表示"]),
];

/** 並びは同じ重みのときの順。ジャンルは「その他」「ツール」のようなありふれた名前が多いので最後にする。 */
export const APP_SEARCH_ENTRIES: readonly AppSearchEntry[] = [
  ...NAV_ENTRIES,
  ...PAGE_ENTRIES,
  ...SETTING_ENTRIES,
  ...genreEntries("asset"),
  ...genreEntries("world"),
];

/**
 * 画面と設定の照合用にそろえる。NFKC（全角英数・半角カナ）と大文字小文字に加え、
 * ひらがなをカタカナに寄せる（「だーく」で「ダーク」を探せる）。
 */
export function foldAppSearchText(text: string): string {
  return text
    .normalize("NFKC")
    .toLocaleLowerCase("ja")
    .replace(/[ぁ-ゖ]/g, (ch) => String.fromCharCode(ch.charCodeAt(0) + 0x60));
}

function appTokens(query: string): string[] {
  return foldAppSearchText(query).trim().split(/\s+/).filter((token) => token.length > 0);
}

/**
 * ライブラリの検索と同じく、空白で区切った語をすべて含む項目（AND）。題が検索語とちょうど同じもの、
 * 題に含まれる語の多いものの順に先へ出し、同じなら索引の並び（サイドバー → 画面 → 設定 → ジャンル）のまま返す。
 * 空の検索語は何も返さない。
 */
export function filterAppEntries(entries: readonly AppSearchEntry[], query: string): AppSearchEntry[] {
  const tokens = appTokens(query);
  if (tokens.length === 0) return [];
  const hits: { entry: AppSearchEntry; score: number; order: number }[] = [];
  entries.forEach((entry, order) => {
    const haystack = foldAppSearchText([entry.title, ...entry.path, ...(entry.keywords ?? [])].join("\n"));
    if (!tokens.every((token) => haystack.includes(token))) return;
    const title = foldAppSearchText(entry.title);
    const exact = title === tokens.join(" ") ? tokens.length + 1 : 0;
    hits.push({ entry, score: exact || tokens.filter((token) => title.includes(token)).length, order });
  });
  return hits.sort((a, b) => b.score - a.score || a.order - b.order).map((hit) => hit.entry);
}

/** 今の状態ではその項目が画面に出ないときの注記。出るとき、または分からないときは null。 */
export function appEntryNote(entry: AppSearchEntry, context: AppSearchContext): string | null {
  if (entry.requires === "builtinBrowser" && !context.builtinBrowser) return "内蔵ブラウザを有効にすると表示されます";
  if (entry.requires === "extension" && context.extension === false) return "読み込み方で拡張機能を選ぶと表示されます";
  return null;
}

/** 移る先の要素。画面に出ない項目は親（`fallback`）へ。 */
export function appEntryTargets(entry: AppSearchEntry, context: AppSearchContext): { target?: string; fallback?: string } {
  if (appEntryNote(entry, context)) return { target: entry.fallback };
  return { target: entry.target, fallback: entry.fallback };
}
