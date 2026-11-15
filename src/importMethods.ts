import type { BoothImportMode } from "./types";

/** 読み込み方の名前と説明。選ぶ画面の列もこの順（おすすめの内蔵ブラウザを先頭）に並べる。 */
export type ImportMethod = {
  mode: BoothImportMode;
  name: string;
  /** 名前の下に添える短い補足。 */
  sub: string;
  summary: string;
  /** 確定するボタンの文言。確かめる画面の見出しは「〜前に」。 */
  action: string;
};

export const IMPORT_METHODS: readonly ImportMethod[] = [
  {
    mode: "browser",
    name: "内蔵ブラウザ",
    sub: "アプリの中で",
    summary: "アプリの右パネルで BOOTH にログインして読み込みます。拡張機能は要りません。",
    action: "内蔵ブラウザにする",
  },
  {
    mode: "extension",
    name: "拡張機能",
    sub: "Chrome / Edge",
    summary: "普段のブラウザに VRCLibMan の拡張機能を入れ、BOOTH のページからアプリへ送ります。アプリにログイン状態を置きません。",
    action: "拡張機能にする",
  },
  {
    mode: "both",
    name: "両方",
    sub: "使い分け",
    summary: "内蔵ブラウザと拡張機能を使い分けます。ログイン状態はそれぞれ別です。",
    action: "両方使う",
  },
];

export function importMethod(mode: BoothImportMode): ImportMethod {
  return IMPORT_METHODS.find((method) => method.mode === mode) ?? IMPORT_METHODS[0];
}

export type ComparisonCell = { can: boolean; note?: string };
/** `emphasis` は選ぶ前に必ず見てほしい行。表の先頭に置き、太字にして、× を注意の色にする。 */
export type ComparisonRow = { label: string; emphasis?: boolean } & Record<BoothImportMode, ComparisonCell>;

const yes = (note?: string): ComparisonCell => ({ can: true, note });
const no = (note?: string): ComparisonCell => ({ can: false, note });

/**
 * 読み込み方を選ぶ画面の比較表。事実は docs/browser-extension.md の「内蔵ブラウザとの違い」と合わせる。
 * 「両方」はたいてい両者を合わせたものだが、ログインと準備の行はそうならないので、列ごとに書く。
 * ログイン状態の保存は、内蔵ブラウザを選ぶかどうかを分ける点なので、先頭に置いて強調する。
 */
export const IMPORT_COMPARISON: ReadonlyArray<{ group: string; rows: readonly ComparisonRow[] }> = [
  {
    group: "ログイン",
    rows: [
      {
        label: "ログイン状態をアプリに保存しない",
        emphasis: true,
        browser: no("アプリに保存する"),
        extension: yes(),
        both: no("内蔵ブラウザ分を保存"),
      },
    ],
  },
  {
    group: "商品の読み込み",
    rows: [
      { label: "ライブラリの商品をまとめて読み込む", browser: yes(), extension: yes(), both: yes() },
      { label: "購入履歴（注文日と金額）を読み込む", browser: yes(), extension: yes(), both: yes() },
      { label: "買った商品を自動で登録する", browser: yes("内蔵ブラウザで購入"), extension: yes("普段のブラウザで購入"), both: yes() },
    ],
  },
  {
    group: "ファイル",
    rows: [
      { label: "アプリの商品カードからダウンロードする", browser: yes(), extension: no(), both: yes() },
      { label: "BOOTH のページで選んだファイルを取得して整理する", browser: yes(), extension: yes(), both: yes() },
      { label: "アセット更新の「今すぐ確認」でライブラリを読み直す", browser: yes(), extension: no(), both: yes() },
    ],
  },
  {
    group: "使う場所と準備",
    rows: [
      { label: "BOOTH のページに、持っているかと対応アバターを出す", browser: no(), extension: yes(), both: yes() },
      { label: "拡張機能を入れずに使える", browser: yes(), extension: no(), both: no() },
      { label: "普段のブラウザのまま使える", browser: no(), extension: yes(), both: yes() },
    ],
  },
];

/** `warn` は読み飛ばされては困る項目。見出しを注意の色にし、本文を濃く出す。 */
export type NoteGroup = { title: string; items: readonly string[]; tone?: "warn" };

/**
 * 内蔵ブラウザを使い始める前の確認。1 段落の OS のダイアログでは読まれないので、項目ごとに分けて出す。
 * 「おすすめ」と注意が同時に出て矛盾して見えないよう、何を保存し、いつ使い、何をしないかを並べる。
 */
export const BUILTIN_BROWSER_CONFIRM: readonly NoteGroup[] = [
  {
    title: "保存するもの",
    items: ["内蔵ブラウザで BOOTH / pixiv にログインした状態を、この PC のアプリ専用の場所に保存します。パスワードはアプリでは扱いません。"],
  },
  {
    title: "使う場面",
    items: [
      "「ライブラリ全取得」を押したときだけ、ライブラリのページを 1 ページずつ読みます。",
      "ファイルの「ダウンロード」を押したときだけ、その 1 件の BOOTH への要求にログイン状態（Cookie）を使います。開発者のサーバーや別のサイトには送りません。",
    ],
  },
  {
    title: "しないこと",
    items: ["購入、一括のダウンロード、年齢確認の自動通過はしません。"],
  },
  {
    title: "注意",
    tone: "warn",
    items: ["VRCLibMan は BOOTH の非公式アプリです。ログインした状態で BOOTH を読み込むため、アカウントが制限・停止される可能性があります。アプリは、あなたのアカウントの安全を保証できません。"],
  },
];

/**
 * 拡張機能を使い始めるときの注意。拡張機能もライブラリと購入履歴はブラウザのログインで読むので、内蔵ブラウザと
 * 同じく、アカウントの安全を保証できないことを伝える（内蔵ブラウザの注意と一緒に出るときは重ねない）。
 */
export const EXTENSION_CAUTION: NoteGroup = {
  title: "注意",
  tone: "warn",
  items: ["VRCLibMan は BOOTH の非公式アプリです。拡張機能も、ライブラリや購入履歴を読むときはブラウザの BOOTH のログインを使うため、アカウントが制限・停止される可能性があります。アプリは、あなたのアカウントの安全を保証できません。"],
};

/**
 * 切り替えたときに起きることを、確かめる画面に出す順で返す。`from` が null なら、まだ何も選んでいない
 * （初回セットアップ。保存値は既定の拡張機能だが、本人が選んだものとしては扱わない）。
 * 受信口は読み込み方に関係なく開いているので、内蔵ブラウザだけにしても、入れてある拡張機能は動き続ける。
 */
export function switchNotes(from: BoothImportMode | null, to: BoothImportMode): NoteGroup[] {
  const builtinOn = from === "browser" || from === "both";
  const extensionOn = from === "extension" || from === "both";
  const notes: NoteGroup[] = [];
  if (to !== "extension" && !builtinOn) notes.push(...BUILTIN_BROWSER_CONFIRM);
  if (to !== "browser" && !extensionOn) {
    notes.push({
      title: "このあとすること",
      items: [
        "選ぶと、この下に拡張機能のフォルダとペアリングコードが出ます。",
        "Chrome / Edge の拡張機能の画面でデベロッパーモードをオンにし、そのフォルダを読み込みます。",
        "拡張機能のボタンを押し、ペアリングコードを貼ってつなぎます。",
      ],
    });
  }
  if (to === "extension" && builtinOn) {
    notes.push({
      title: "内蔵ブラウザについて",
      items: [
        "内蔵ブラウザを閉じます。商品カードの「ダウンロード」とアセット更新の「今すぐ確認」は使えなくなります。",
        "ログイン状態は消えず、内蔵ブラウザに戻すとそのまま使えます。消したいときは、切り替える前に「内蔵ブラウザの閲覧データを削除」を押してください。",
      ],
    });
  }
  if (to === "browser" && extensionOn) {
    notes.push({
      title: "拡張機能について",
      items: [
        "拡張機能の手順とペアリングコードは、設定に出なくなります。",
        "ブラウザに入れた拡張機能は自動では外れません。使わないときは、ブラウザの拡張機能の画面で削除してください。",
      ],
    });
  }
  if (to !== "browser" && !extensionOn && !notes.some((group) => group.tone === "warn")) notes.push(EXTENSION_CAUTION);
  return notes;
}
