import {
  copyFileSync,
  mkdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function stageUpdaterArtifacts({ target, paths, destination, pubkey }) {
  const extension =
    target === "windows-x64"
      ? "-setup.exe"
      : target === "macos-aarch64"
        ? ".app.tar.gz"
        : null;
  if (!extension) throw new Error("更新ファイルの対象が正しくありません");
  const packages = paths.filter((path) => path.endsWith(extension));
  if (packages.length !== 1)
    throw new Error(`${target} の更新ファイルを一つに特定できません`);
  const artifact = packages[0];
  const signature = `${artifact}.sig`;
  if (!paths.includes(signature))
    throw new Error(`${target} の署名ファイルがありません`);
  if (!pubkey?.trim()) throw new Error("更新ファイルの公開鍵がありません");
  for (const path of [artifact, signature]) {
    const stat = statSync(path);
    if (!stat.isFile() || stat.size === 0)
      throw new Error(`更新ファイルが空です: ${basename(path)}`);
  }
  mkdirSync(destination, { recursive: true });
  for (const path of [artifact, signature])
    copyFileSync(path, resolve(destination, basename(path)));
  writeFileSync(
    resolve(destination, "updater-public-key.txt"),
    pubkey.trim() + "\n",
  );
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    const [target, destination, configPath] = process.argv.slice(2);
    if (!configPath)
      throw new Error("target destination config.json を指定してください");
    stageUpdaterArtifacts({
      target,
      destination,
      paths: JSON.parse(process.env.ARTIFACT_PATHS || "[]"),
      pubkey: JSON.parse(readFileSync(configPath, "utf8")).plugins?.updater
        ?.pubkey,
    });
    console.log(`${target}: 更新ファイル・署名・公開鍵を検証用に保存しました`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
