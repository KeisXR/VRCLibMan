import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

export function createReleaseNotes({ appVersion, extensionVersion, template, changes }) {
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(appVersion ?? "")) {
    throw new Error("アプリの版が不正です");
  }
  if (!/^\d+(?:\.\d+){0,3}$/.test(extensionVersion ?? "")) {
    throw new Error("拡張の版が不正です");
  }
  if (!changes?.trim()) {
    throw new Error(`アプリ ${appVersion} の変更内容を記載してください`);
  }
  const reclassifyMatch = changes.match(/^## 再分類のおすすめ\s*\n+([\s\S]*?)(?=\n## |\s*$)/m);
  const changesBody = reclassifyMatch
    ? changes.replace(/^## 再分類のおすすめ\s*\n+[\s\S]*?(?=\n## |\s*$)/m, "").trim()
    : changes.trim();
  const reclassifyNote = reclassifyMatch?.[1]?.trim() || "（この版では分類ルールの変更はありません）";
  const replacements = {
    APP_VERSION: appVersion,
    EXTENSION_VERSION: extensionVersion,
    CHANGES: changesBody,
    RECLASSIFY_NOTE: reclassifyNote,
  };
  for (const key of Object.keys(replacements)) {
    if (!template.includes(`{{${key}}}`)) {
      throw new Error(`Release 雛形に ${key} がありません`);
    }
  }
  return template.replace(/{{([A-Z_]+)}}/g, (placeholder, key) => {
    if (!(key in replacements)) throw new Error(`Release 雛形の項目が不明です: ${key}`);
    return replacements[key];
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [appVersion, extensionVersion, destination] = process.argv.slice(2);
    // Validate before using the version as part of a file name.
    if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(appVersion ?? "")) {
      throw new Error("アプリの版が不正です");
    }
    if (!destination) throw new Error("Release 本文の出力先を指定してください");
    const changesPath = new URL(`../.github/release-notes/${appVersion}.md`, import.meta.url);
    let changes;
    try {
      changes = readFileSync(changesPath, "utf8");
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
      throw new Error(`.github/release-notes/${appVersion}.md にこの版の変更内容を記載してください`);
    }
    const template = readFileSync(new URL("../.github/release-notes.md", import.meta.url), "utf8");
    writeFileSync(destination, createReleaseNotes({ appVersion, extensionVersion, template, changes }));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
