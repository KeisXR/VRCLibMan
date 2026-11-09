// Checks that every place carrying the app version agrees, and (when given) that the
// release tag is "v" + that version. The Chrome extension keeps its own version in
// extension/manifest.json; it is only checked for a valid Chrome version format.
//
// usage: node scripts/check-release-version.mjs [--tag vX.Y.Z]
// Writes app_version / extension_version to $GITHUB_OUTPUT when it is set.
import { appendFileSync, readFileSync } from "node:fs";

const args = process.argv.slice(2);
let tag = "";
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === "--tag") tag = args[++i] ?? "";
  else {
    console.error(`unknown argument ${args[i]}`);
    process.exit(2);
  }
}

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const cargoToml = readFileSync("src-tauri/Cargo.toml", "utf8");
const packageSection = cargoToml.split(/^\[/m).find((section) => section.startsWith("package]")) ?? "";
const crateName = packageSection.match(/^name\s*=\s*"([^"]+)"/m)?.[1];
const cargoLock = readFileSync("src-tauri/Cargo.lock", "utf8");
const lockVersion = crateName
  ? cargoLock
      .split("[[package]]")
      .find((block) => block.match(/^name = "([^"]+)"/m)?.[1] === crateName)
      ?.match(/^version = "([^"]+)"/m)?.[1]
  : undefined;

const sources = {
  "package.json": readJson("package.json").version,
  "package-lock.json": readJson("package-lock.json").packages?.[""]?.version,
  "src-tauri/tauri.conf.json": readJson("src-tauri/tauri.conf.json").version,
  "src-tauri/Cargo.toml": packageSection.match(/^version\s*=\s*"([^"]+)"/m)?.[1],
  "src-tauri/Cargo.lock": lockVersion,
};
const extensionVersion = readJson("extension/manifest.json").version;

const errors = [];
for (const [file, version] of Object.entries(sources)) {
  console.log(`${file}: ${version ?? "(not found)"}`);
  if (!version) errors.push(`${file} にアプリの版がありません`);
}
console.log(`extension/manifest.json: ${extensionVersion ?? "(not found)"} (拡張は独立した版。タグとは照合しない)`);

const appVersion = sources["package.json"];
const distinct = new Set(Object.values(sources).filter(Boolean));
if (distinct.size > 1) {
  errors.push(
    `アプリの版が揃っていません: ${Object.entries(sources)
      .map(([file, version]) => `${file}=${version}`)
      .join(", ")}`,
  );
}
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(appVersion ?? "")) {
  errors.push(`アプリの版 ${appVersion} は X.Y.Z 形式ではありません`);
}
if (!/^\d+(?:\.\d+){0,3}$/.test(extensionVersion ?? "")) {
  errors.push(`拡張の版 ${extensionVersion} は Chrome の版の形式（数字を . で 1〜4 個）ではありません`);
}
if (tag && tag !== `v${appVersion}`) {
  errors.push(`タグ ${tag} がアプリの版と一致しません。v${appVersion} を打つか、版を ${tag.replace(/^v/, "")} に上げてください`);
}

if (errors.length > 0) {
  for (const error of errors) console.error(`::error::${error}`);
  process.exit(1);
}

if (process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT, `app_version=${appVersion}\nextension_version=${extensionVersion}\n`);
}
console.log(tag ? `OK: ${tag} = アプリ ${appVersion}` : `OK: アプリ ${appVersion}（タグ照合なし）`);
