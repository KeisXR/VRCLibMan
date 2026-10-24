export const MODEL_CHOICES = [
  {
    name: "qwen3.5:4b",
    label: "軽量・推奨",
    size: "約 3.3〜4.0 GB",
    memory: "8 GB 以上",
  },
  {
    name: "qwen3.5:9b",
    label: "精度寄り",
    size: "約 6.6〜7.6 GB",
    memory: "16 GB 以上",
  },
  {
    name: "gemma4:12b",
    label: "従来の既定",
    size: "約 7.5 GB",
    memory: "16 GB 以上",
  },
] as const;

export function modelInstalled(
  models: readonly string[],
  wanted: string,
): boolean {
  return (
    wanted.trim() !== "" &&
    models.some((name) => name === wanted || name.startsWith(`${wanted}:`))
  );
}

export function modelDownloadPrompt(name: string): string {
  const choice = MODEL_CHOICES.find((item) => item.name === name);
  return `${name} を Ollama からダウンロードします。${choice ? `サイズの目安は ${choice.size} です。` : "サイズはモデルにより異なり、数 GB 以上になることがあります。"}続けますか？`;
}
