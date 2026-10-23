function pageNotes() {
  try {
    const products = globalThis.VrcLibManExtract.extractPage(document, location.href);
    return (products || [])
      .filter((product) => product && product.boothId)
      .map((product) => ({
        boothId: String(product.boothId),
        isFree: product.isFree === true,
        isAdult: product.isAdult === true,
        title: String(product.title || "").slice(0, 300),
      }));
  } catch {
    return [];
  }
}

function rememberPage() {
  const notes = pageNotes();
  if (notes.length === 0 || !chrome.storage || !chrome.storage.session) return;
  chrome.storage.session.get({ pageNotes: [] }, (data) => {
    const map = new Map((data.pageNotes || []).filter((row) => row && row.boothId).map((row) => [row.boothId, row]));
    for (const note of notes) map.set(note.boothId, note);
    const merged = [...map.values()].slice(-40);
    chrome.storage.session.set({ pageNotes: merged });
  });
}

rememberPage();

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!message || message.type !== "extract-page") return undefined;
  try {
    const products = globalThis.VrcLibManExtract.extractPage(document, location.href);
    rememberPage();
    sendResponse({ products });
  } catch (error) {
    sendResponse({ error: String(error) });
  }
  return true;
});
