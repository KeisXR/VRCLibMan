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
  function labels(row) {
    if (!row?.registered) return ['未登録'];
    return [row.owned ? '所持確認済み' : '登録済み（所持未確認）', row.localRecorded ? '保存記録あり' : row.filesKnown ? '保存記録なし' : 'ファイル情報未取得'];
  }
  function matches(row, avatar) {
    // Unknown compatibility is retained, never presented as incompatible.
    return !avatar || !row?.avatars?.length || row.avatars.some(a => a.id === avatar);
  }
  return { boothUrl, downloadId, choose, labels, matches };
});
