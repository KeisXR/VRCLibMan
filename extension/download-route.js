(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) {
    module.exports = api;
  }
  root.VrcLibManRoute = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const ASSET = /\.(unitypackage|zip|rar|7z|vrm)$/i;

  function clean(value) {
    return String(value || "").replace(/\s+/g, " ").trim();
  }

  function safeDownloadName(value) {
    const text = clean(String(value || "").split(/[?#]/)[0].split(/[\\/]/).pop());
    if (!text || text.length > 180 || text === "." || text === "..") return "";
    if (/[\\/:*?"<>|\u0000]/.test(text)) return "";
    return text;
  }

  function boothHost(value) {
    try {
      const url = new URL(String(value || ""));
      return url.protocol === "https:" && !url.username && !url.password && !url.port
        && /(^|\.)booth\.pm$/.test(url.hostname);
    } catch { return false; }
  }

  function pathOf(value) {
    try { return new URL(String(value || "")).pathname; } catch { return ""; }
  }

  // BOOTH's files start at a BOOTH address (booth.pm/downloadables/<n>) and are then redirected.
  // The page that linked to a file (referrer) does not make another site's file a BOOTH file.
  function isBoothAsset(item) {
    if (!item) return false;
    const name = safeDownloadName(item.filename || item.finalUrl || item.url || "");
    if (!ASSET.test(name)) return false;
    return boothHost(item.url);
  }

  // Whoever writes a link chooses its query string, and the page it sits on. So the product ID
  // is read from paths only, and only from BOOTH's own addresses: the start of the download,
  // where BOOTH's redirect ended, and a BOOTH page that linked to it.
  function boothIdFrom(item) {
    const started = Boolean(item) && boothHost(item.url);
    const blob = [
      started ? pathOf(item.url) : "",
      item && (started || boothHost(item.finalUrl)) ? pathOf(item.finalUrl) : "",
      item && boothHost(item.referrer) ? pathOf(item.referrer) : "",
      item && item.filename,
    ].join("\n");
    const itemId = blob.match(/\/items\/(\d{4,12})/);
    if (itemId) return itemId[1];
    const downloadId = blob.match(/\/downloads\/(\d{4,12})/);
    if (downloadId) return downloadId[1];
    const staged = blob.match(/VRCLibMan[\\/](\d{4,12})[\\/]/i);
    return staged ? staged[1] : "";
  }

  function decideFilename(item, enabled) {
    if (enabled !== true) return null;
    if (!isBoothAsset(item)) return null;
    const id = boothIdFrom(item);
    if (!id) return null;
    const name = safeDownloadName(item.filename || item.finalUrl || item.url || "");
    if (!name) return null;
    return { filename: `VRCLibMan/${id}/${name}`, conflictAction: "uniquify" };
  }

  function completionNotice(item) {
    if (!item) return null;
    const full = String(item.filename || "");
    if (!/(^|[\\/])VRCLibMan([\\/]|$)/i.test(full)) return null;
    const id = boothIdFrom(item);
    const name = safeDownloadName(full);
    if (!id || !name) return null;
    const size = Number(item.fileSize);
    return {
      boothId: id,
      filename: name,
      path: full,
      size: Number.isFinite(size) && size >= 0 ? size : null,
      variation: "",
    };
  }

  return {
    decideFilename,
    completionNotice,
    isBoothAsset,
    boothIdFrom,
    safeDownloadName,
  };
});
