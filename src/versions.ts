export function extensionVersionMessage(
  expected: string,
  actual: string | null,
): string {
  if (!actual)
    return "バージョン未確認。拡張のポップアップで確認できます。";
  return expected === actual
    ? "バージョン一致"
    : "バージョン不一致。「BOOTH → 拡張機能をつなぐ」のフォルダから拡張機能を読み込み直してください。";
}
