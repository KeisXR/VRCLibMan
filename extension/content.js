chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!message || message.type !== "extract-page") return undefined;
  try {
    const products = globalThis.VrcLibManExtract.extractPage(document, location.href);
    sendResponse({ products });
  } catch (error) {
    sendResponse({ error: String(error) });
  }
  return true;
});
