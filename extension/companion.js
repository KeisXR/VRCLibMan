// Browser-only helpers. Library ownership and avatar relationships come from Rust.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  root.VrcLibManCompanion = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  function boothUrl(value) {
    try { const u = new URL(value); return u.protocol === 'https:' && !u.username && !u.password && !u.port && /(^|\.)booth\.pm$/.test(u.hostname) ? u : null; } catch { return null; }
  }
  function downloadId(value) {
    const u = boothUrl(value);
    return u && /^\/downloadables\/(\d{4,12})\/?$/.exec(u.pathname)?.[1] || '';
  }
  function choose(products, id) {
    if (!/^\d{4,12}$/.test(id || '')) return null;
    const rows = (products || []).flatMap(product => (product.downloads || [])
      .filter(file => file.downloadableId === id)
      .map(file => ({ product, file })));
    return rows.length === 1 ? rows[0] : null;
  }
  // The first product with this ID on the page. Pages list each product once after dedupe.
  function productById(products, id) {
    if (!/^\d{4,12}$/.test(id || '')) return null;
    return (products || []).find(product => product && String(product.boothId) === id) || null;
  }
  // Short state marks. 「所持」 means an owned variation was imported; it is not proof of a paid purchase.
  function chips(row) {
    if (!row?.registered) return [{ text: '未登録', tone: 'warn' }];
    const result = [row.owned ? { text: '所持', tone: 'good' } : { text: '登録のみ', tone: '' }];
    if (row.localRecorded) result.push({ text: '保存記録あり', tone: 'good' });
    else result.push({ text: row.filesKnown ? '保存記録なし' : 'ファイル情報未取得', tone: '' });
    return result;
  }
  function labels(row) {
    return chips(row).map(chip => chip.text);
  }
  function avatarNames(row) {
    return (row?.avatars || []).map(avatar => avatar.name + (avatar.viaBaseBody ? '・共通素体' : ''));
  }
  function saved(row, downloadableId) {
    return Boolean(row?.savedFiles?.includes(downloadableId));
  }
  function matches(row, avatar) {
    // Unknown compatibility is retained, never presented as incompatible.
    return !avatar || !row?.avatars?.length || row.avatars.some(a => a.id === avatar);
  }
  function isAsset(filename) {
    return /\.(unitypackage|zip|rar|7z|vrm|fbx|blend|mp3|wav|ogg|flac|m4a|aac|png|jpg|jpeg|webp|psd|clip)$/i.test(String(filename || ''));
  }
  return { boothUrl, downloadId, choose, productById, chips, labels, avatarNames, saved, matches, isAsset };
});
