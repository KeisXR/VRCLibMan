/**
 * 内蔵ブラウザのアドレス欄の表示。編集していないときは https:// を省き、狭い欄でも
 * ホスト名が先頭に来るようにする。http:// など https 以外はそのまま見せる（安全でない接続を隠さない）。
 * 省いた表示のまま送っても、Rust の navigation_url が https:// を補う。
 */
export function readableAddress(url: string): string {
  const match = /^https:\/\/(.*)$/i.exec(url.trim());
  if (!match) return url;
  const rest = match[1];
  // ホストだけの URL（https://booth.pm/）は末尾の / も省く。
  return /^[^/?#]+\/$/.test(rest) ? rest.slice(0, -1) : rest;
}

