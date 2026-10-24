importScripts("download-route.js");

const LOCAL_BASE = "http://127.0.0.1:17321";

function suggestPlain(suggest, decision) {
  if (decision) suggest(decision);
  else suggest();
}

chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  let settled = false;
  const finish = (decision) => {
    if (settled) return;
    settled = true;
    suggestPlain(suggest, decision);
  };
  try {
    chrome.storage.local.get({ routeDownloads: false }, (data) => {
      if (chrome.runtime.lastError) {
        finish(null);
        return;
      }
      const enabled = data.routeDownloads === true;
      finish(globalThis.VrcLibManRoute.decideFilename(item, enabled));
    });
  } catch {
    finish(null);
  }
  return true;
});

chrome.downloads.onChanged.addListener((delta) => {
  if (!delta.state || delta.state.current !== "complete" || delta.id == null) return;
  chrome.downloads.search({ id: delta.id }, (items) => {
    const item = items && items[0];
    const notice = globalThis.VrcLibManRoute.completionNotice(item);
    if (!notice) return;
    notifyFinished(notice);
  });
});

async function notifyFinished(notice) {
  const stored = await chrome.storage.local.get({ pairingToken: "", routeDownloads: false });
  if (stored.routeDownloads !== true) return;
  const token = String(stored.pairingToken || "").trim();
  if (!token) return;
  const session = await chrome.storage.session.get({ pageNotes: [] });
  const note = (session.pageNotes || []).find((row) => row && row.boothId === notice.boothId);
  const body = {
    boothId: notice.boothId,
    filename: notice.filename,
    path: notice.path,
    size: notice.size,
    variation: notice.variation || "",
  };
  if (note && note.isFree === true) body.isFree = true;
  if (note && note.isAdult === true) body.isAdult = true;
  if (note && note.title) body.title = String(note.title).slice(0, 300);
  try {
    await fetch(`${LOCAL_BASE}/download-finished`, {
      method: "POST",
      credentials: "omit",
      headers: {
        Authorization: `Bearer ${token}`, "X-VRCLibMan-Extension-Version": chrome.runtime.getManifest().version,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    });
  } catch {
    // アプリが閉じているときは、Downloads/VRCLibMan に残す。
  }
}
