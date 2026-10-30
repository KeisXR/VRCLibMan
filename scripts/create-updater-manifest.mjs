import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Run once after all platform builds finish; parallel builds never overwrite latest.json.
export function createUpdaterManifest({
  version,
  repository,
  assets,
  signature,
  notes,
  now = new Date(),
}) {
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version))
    throw new Error("アプリの版が正しくありません");
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository))
    throw new Error("リポジトリ名が正しくありません");
  const names = {
    "windows-x86_64": `VRCLibMan_${version}_x64-setup.exe`,
    "darwin-aarch64": `VRCLibMan_${version}_aarch64.app.tar.gz`,
    "linux-x86_64-appimage": `VRCLibMan_${version}_amd64.AppImage`,
    "linux-aarch64-appimage": `VRCLibMan_${version}_aarch64.AppImage`,
  };
  const platforms = {};
  for (const [target, name] of Object.entries(names)) {
    const artifact = assets.find(
      (asset) =>
        asset.name === name && asset.state === "uploaded" && asset.size > 0,
    );
    const sigAsset = assets.find(
      (asset) =>
        asset.name === `${name}.sig` &&
        asset.state === "uploaded" &&
        asset.size > 0,
    );
    if (!artifact || !sigAsset)
      throw new Error(`${target} の更新ファイルまたは署名がありません`);
    const expectedUrl = `https://github.com/${repository}/releases/download/v${version}/${name}`;
    // GitHub uses an untagged-* download path while a release is still a draft.
    // Publish changes it to the version tag; clients must receive that final URL.
    const draftPrefix = `https://github.com/${repository}/releases/download/`;
    const draftPath = artifact.browser_download_url?.startsWith(draftPrefix)
      ? artifact.browser_download_url.slice(draftPrefix.length).split("/")
      : [];
    const validDraftUrl =
      draftPath.length === 2 &&
      /^untagged-[a-f0-9]+$/i.test(draftPath[0]) &&
      draftPath[1] === name;
    if (artifact.browser_download_url !== expectedUrl && !validDraftUrl)
      throw new Error(`${target} の更新先 URL が一致しません`);
    const signed = signature(`${name}.sig`).trim();
    const decoded = Buffer.from(signed, "base64").toString("utf8");
    if (
      !/^[A-Za-z0-9+/]+={0,2}$/.test(signed) ||
      !decoded.startsWith("untrusted comment:") ||
      !decoded.includes("\ntrusted comment:")
    ) {
      throw new Error(`${target} の署名の形式が正しくありません`);
    }
    platforms[target] = {
      signature: signed,
      url: expectedUrl,
    };
  }
  return { version, notes, pub_date: now.toISOString(), platforms };
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    const [version, repository, assetsPath, signaturesPath, notesPath, output] =
      process.argv.slice(2);
    if (!output)
      throw new Error(
        "version repository assets.json signatures-dir notes.md output.json を指定してください",
      );
    const manifest = createUpdaterManifest({
      version,
      repository,
      assets: JSON.parse(readFileSync(assetsPath, "utf8")),
      signature: (name) => readFileSync(resolve(signaturesPath, name), "utf8"),
      notes: readFileSync(notesPath, "utf8"),
    });
    writeFileSync(output, JSON.stringify(manifest, null, 2) + "\n");
    console.log(
      `更新情報 v${version}: ${Object.keys(manifest.platforms).join(", ")}`,
    );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
