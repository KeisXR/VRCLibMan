import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import test from 'node:test';
import { parseHTML } from 'linkedom';
const require = createRequire(import.meta.url);
const { checkout, chooseDownload } = require('../src-tauri/src/browser_auto.cjs');
const href = 'https://checkout.booth.pm/orders/123456?uuid=example';
function page(items, { event = 'purchased', title = 'ご注文が完了しました', order = '123456' } = {}) {
  return parseHTML(`<html><head><meta name="xrcloud:event" content="${event}"></head><body>
    <h2 class="page-header__title">${title}</h2><div data-tracking="order" data-order-id="${order}"></div>
    ${items}<aside><a href="https://booth.pm/ja/items/999999">おすすめ商品</a></aside></body></html>`).document;
}
const item = (id, name = '見本の衣装') => `<div data-tracking="item" data-product-id="${id}" data-product-name="${name}" data-product-variant="456789"></div>`;
test('confirmed checkout extracts its own product, excluding recommendations', () => {
  const result = checkout(page(item('123456')), href);
  assert.equal(result.length, 1); assert.equal(result[0].boothId, '123456');
});
test('multiple purchased items and variations deduplicate by product', () => {
  const result = checkout(page(item('123456') + item('234567', '見本の髪') + item('123456')), href);
  assert.deepEqual(result.map(p => p.boothId), ['123456', '234567']);
});
test('pending payment, a mismatched order, missing metadata and other origins do not register', () => {
  for (const options of [{event:'pending'}, {title:'お支払い待ち'}, {order:'111111'}]) assert.deepEqual(checkout(page(item('123456'), options), href), []);
  assert.deepEqual(checkout(page(''), href), []);
  for (const url of ['http://checkout.booth.pm/orders/123456','https://booth.pm/orders/123456','https://checkout.booth.pm.evil.test/orders/123456','https://checkout.booth.pm/orders/123456/other']) assert.deepEqual(checkout(page(item('123456')), url), []);
});
test('invalid records are rejected', () => {
  assert.deepEqual(checkout(page(item('../1234') + item('1234', '')), href), []);
});
test('download requires a unique exact downloadable id; names and recommendations are insufficient', () => {
  const product = {boothId:'123456', downloads:[{downloadableId:'555555',filename:'sample.zip',variation:'通常'}]};
  assert.equal(chooseDownload([product], '555555').variation, '通常');
  assert.equal(chooseDownload([product], '666666'), null);
  assert.equal(chooseDownload([product, product], '555555'), null);
  assert.equal(chooseDownload([product], '../555555'), null);
});
