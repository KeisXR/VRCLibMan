export function extensionVersionMessage(
  expected: string,
  actual: string | null,
): string {
  if (!actual)
    return "バージョン未確認。拡張のポップアップで確認できます。";
  return expected === actual
    ? "バージョン一致"
    : "バージョン不一致。拡張を更新してください。";
}
