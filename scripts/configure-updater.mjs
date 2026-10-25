import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function updaterConfig(env) {
  const pubkey = env.TAURI_UPDATER_PUBLIC_KEY?.trim();
  if (!pubkey || !env.TAURI_SIGNING_PRIVATE_KEY?.trim()) {
    throw new Error(
      "TAURI_UPDATER_PUBLIC_KEY (Actions Variable) と TAURI_SIGNING_PRIVATE_KEY (Actions Secret) を登録してください。docs/updater.md を参照してください。",
    );
  }
  const decoded = Buffer.from(pubkey, "base64").toString("utf8").trim();
  const lines = decoded.split(/\r?\n/);
  if (Buffer.from(pubkey, "base64").toString("base64") !== pubkey) {
    throw new Error(
      "公開鍵は tauri signer generate が出力した .pub ファイルの内容を指定してください。",
    );
  }
  if (
    lines.length !== 2 ||
    !lines[0].startsWith("untrusted comment:") ||
    Buffer.from(lines[1], "base64").length !== 42
  ) {
    throw new Error("更新用の公開鍵の形式が正しくありません。");
  }
  return {
    bundle: { createUpdaterArtifacts: true },
    plugins: { updater: { pubkey } },
  };
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    const output = process.argv[2];
    if (!output) throw new Error("出力先の config JSON を指定してください。");
    writeFileSync(
      output,
      JSON.stringify(updaterConfig(process.env), null, 2) + "\n",
    );
    console.log(
      "更新の署名設定を確認しました。公開鍵だけをビルド設定へ渡します。",
    );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
