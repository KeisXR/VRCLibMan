import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { parseHTML } from "linkedom";

const require = createRequire(import.meta.url);
const { extractPage, trimDescription, cardImage } = require("../extension/extract.cjs");

const ITEM_HTML = `<!doctype html>
<html lang="ja"><head>
  <script type="application/ld+json">
    {"@type":"Product","name":"短い紹介だけの名前","brand":{"@type":"Brand","name":"こまど"},
     "image":"https://booth.pximg.net/c/72x72_a/sample/i/4876459/thumb.jpg",
     "description":"JSON-LD の短い紹介"}
  </script>
  <meta property="og:image" content="https://booth.pximg.net/c/72x72_a/sample/i/4876459/thumb.jpg" />
</head><body>
  <h2>ライム -Lime- / オリジナル3Dモデル</h2>
  <section class="shop__text">
    <h2>商品説明</h2>
    <p class="js-autolink">短い紹介です。</p>
    <h2>素体について</h2>
    <p class="js-autolink">ライム、プラム、シフォン、ショコラは共通素体です。衣装を着せ替えできます。</p>
  </section>
  <section class="shop__text">
    <h2>利用規約</h2>
    <p>再配布はできません。</p>
  </section>
  <div class="item-card">
    <a href="/ja/items/9990001">おすすめの別商品</a>
    <img src="https://booth.pximg.net/c/72x72/other.jpg" alt="別商品" />
  </div>
</body></html>`;

const LIBRARY_HTML = `<!doctype html>
<html><body>
  <div class="bg-white">
    <a href="https://booth.pm/ja/items/5058077">
      <img class="l-library-item-thumbnail" alt="" src="https://booth.pximg.net/c/72x72/manuka.jpg" />
      オリジナル3Dモデル「マヌカ」
    </a>
    <span class="text-text-gray600">STUDIO JINGO</span>
  </div>
  <div class="bg-white">
    <a href="/ja/items/6106863">
      <img class="l-library-item-thumbnail" alt="" src="https://booth.pximg.net/c/72x72/shinano.jpg" />
      しなの
    </a>
    <a class="item-card__shop-name-anchor" href="/shops/1">ポンデロニウム研究所</a>
  </div>
  <a href="/ja/items/1111111">おすすめに紛れたリンク</a>
</body></html>`;

test("item page returns only the main product and keeps the body note", () => {
  const { document } = parseHTML(ITEM_HTML);
  const products = extractPage(document, "https://booth.pm/ja/items/4876459");
  assert.equal(products.length, 1);
  assert.equal(products[0].boothId, "4876459");
  assert.equal(products[0].title, "ライム -Lime- / オリジナル3Dモデル");
  assert.equal(products[0].shop, "こまど");
  assert.match(products[0].descriptionSnippet, /共通素体/);
  assert.equal(products[0].descriptionSnippet.includes("おすすめの別商品"), false);
  assert.match(products[0].thumbnailUrl, /\/c\/360x360\//);
});

test("library page uses item thumbnails and ignores stray links", () => {
  const { document } = parseHTML(LIBRARY_HTML);
  const products = extractPage(document, "https://booth.pm/library");
  assert.deepEqual(products.map((product) => product.boothId), ["5058077", "6106863"]);
  assert.equal(products[0].shop, "STUDIO JINGO");
  assert.equal(products[1].shop, "ポンデロニウム研究所");
});

test("other booth pages export nothing", () => {
  const { document } = parseHTML("<html><body><a href='/ja/items/4876459'>ライム</a></body></html>");
  assert.deepEqual(extractPage(document, "https://booth.pm/ja"), []);
});

test("long descriptions keep compatibility notes and drop terms", () => {
  const filler = "一般的な紹介です。".repeat(1200);
  const text = `${filler}\n\n利用規約\n長い規約です。\n\n素体について\nライム対応の衣装です。`;
  const trimmed = trimDescription(text);
  assert.ok(trimmed.length <= 8000);
  assert.match(trimmed, /ライム対応/);
  assert.equal(trimmed.includes("利用規約"), false);
});

test("card images are rewritten to 360px", () => {
  assert.equal(
    cardImage("https://booth.pximg.net/abcdef/i/4876459/base.jpg"),
    "https://booth.pximg.net/c/360x360/abcdef/i/4876459/base.jpg",
  );
});
