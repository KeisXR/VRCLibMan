const UNSAFE_NAME = /[\u0000-\u001f\u007f<>:"/\\|?*]/g;

/** 既定の保存名。日付は端末の現地日付で、Windows で使えない文字は `-` にする。 */
export function syncExportFilename(deviceName: string, date: Date): string {
  const name = deviceName.trim().replace(UNSAFE_NAME, "-") || "device";
  const year = String(date.getFullYear()).padStart(4, "0");
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `vrclibman-migration-${name}-${year}${month}${day}.zip`;
}
