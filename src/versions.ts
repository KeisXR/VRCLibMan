export function extensionVersionMessage(
  expected: string,
  actual: string | null,
): string {
  if (!actual)
    return "通信した拡張の版は未確認です。拡張のポップアップで確認できます。";
  return expected === actual
    ? "想定する拡張の版と一致しています。"
    : "想定する版と異なります。拡張を更新してください。";
}
