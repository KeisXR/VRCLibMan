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
    return /https?:\/\/([^/\s]+\.)?booth\.pm\//i.test(String(value || ""));
  }

  function isBoothAsset(item) {
    if (!item) return false;
    const name = safeDownloadName(item.filename || item.finalUrl || item.url || "");
    if (!ASSET.test(name)) return false;
    return [item.url, item.finalUrl, item.referrer].some((value) => boothHost(value));
  }

  function boothIdFrom(item) {
    const blob = [item && item.url, item && item.finalUrl, item && item.referrer, item && item.filename].join("\n");
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
