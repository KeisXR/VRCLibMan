import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { parseHTML } from "linkedom";

const require = createRequire(import.meta.url);
const {
  extractPage,
  trimDescription,
  cardImage,
  detailFromItemJson,
  mergeDetail,
  libraryLastPage,
  libraryPageUrl,
  dedupeProducts,
  takeUntilKnown,
  isLibraryUrl,
  quietHtml,
  noteIfUnpublished,
  diagnose,
  UNAVAILABLE_TEXT,
} = require("../extension/extract.js");

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
  <div id="list">
    <div class="bg-white">
      <a href="https://booth.pm/ja/items/5058077">
        <img class="l-library-item-thumbnail" alt="" src="https://booth.pximg.net/c/72x72/manuka.jpg" />
      </a>
      <a class="text-text-default" href="https://booth.pm/items/5058077">オリジナル3Dモデル「マヌカ」</a>
      <span class="text-text-gray600">STUDIO JINGO</span>
      <a href="https://booth.pm/downloads/5058077/file">manuka.zip</a>
    </div>
    <div class="bg-white">
      <img class="l-library-item-thumbnail" alt="しなの" src="https://booth.pximg.net/c/72x72/shinano.jpg" />
      <a href="/ja/items/6106863">しなの</a>
      <a class="item-card__shop-name-anchor" href="/shops/1">ポンデロニウム研究所</a>
    </div>
    <div class="bg-white">
      <a href="/ja/items/4876459">
        <img class="l-library-item-thumbnail" alt="" src="https://booth.pximg.net/c/72x72/lime.jpg" />
        ライム
      </a>
    </div>
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
  assert.match(products[0].thumbnailUrl, /\/c\/150x150\//);
});

test("library page uses item thumbnails and ignores stray links", () => {
  const { document } = parseHTML(LIBRARY_HTML);
  const products = extractPage(document, "https://booth.pm/library");
  assert.deepEqual(products.map((product) => product.boothId), ["5058077", "6106863", "4876459"]);
  assert.equal(products[0].title, "オリジナル3Dモデル「マヌカ」");
  assert.equal(products[0].shop, "STUDIO JINGO");
  assert.equal(products[1].shop, "ポンデロニウム研究所");
});

test("item page without shop sections still reads the description and shop", () => {
  const html = `<!doctype html><body>
    <h2>部屋着サンプル</h2>
    <div class="shop-name">サンプル工房</div>
    <div class="js-market-item-detail-description description"><p>ライム対応の衣装です。</p></div>
    <div class="item-card"><span class="shop-name">別の店</span><a href="/ja/items/9990001">別商品</a></div>
  </body>`;
  const { document } = parseHTML(html);
  const products = extractPage(document, "https://booth.pm/ja/items/1001");
  assert.equal(products.length, 1);
  assert.equal(products[0].shop, "サンプル工房");
  assert.match(products[0].descriptionSnippet, /ライム対応/);
  assert.equal(products[0].descriptionSnippet.includes("別商品"), false);
});

test("item json fills an empty shop and description without replacing the title", () => {
  const product = {
    boothId: "1001",
    title: "部屋着サンプル (Full set)",
    shop: "",
    descriptionSnippet: "",
    thumbnailUrl: "https://booth.pximg.net/c/150x150/sample.jpg",
  };
  const detail = detailFromItemJson(
    {
      id: 1001,
      name: "部屋着サンプル",
      description: "対応アバター\nライム_ver1.00",
      shop: { name: "サンプル工房" },
      url: "https://booth.pm/ja/items/1001",
    },
    "1001",
  );
  mergeDetail(product, detail);
  assert.equal(product.title, "部屋着サンプル (Full set)");
  assert.equal(product.shop, "サンプル工房");
  assert.match(product.descriptionSnippet, /ライム_ver1\.00/);
  assert.equal(product.thumbnailUrl, "https://booth.pximg.net/c/150x150/sample.jpg");
});

test("library pager reports the last page and drops ids already seen", () => {
  const html = `<!doctype html><body>
    <a href="?page=2">2</a>
    <a href="/library?page=3">3</a>
    <a class="last-page" href="https://accounts.booth.pm/library?page=12">最後</a>
  </body>`;
  const { document } = parseHTML(html);
  assert.equal(libraryLastPage(document, "https://accounts.booth.pm/library"), 12);
  assert.equal(
    libraryPageUrl("https://accounts.booth.pm/library?page=4", 1),
    "https://accounts.booth.pm/library?page=1",
  );
  assert.equal(isLibraryUrl("https://accounts.booth.pm/library?page=2"), true);
  assert.equal(isLibraryUrl("https://booth.pm/ja/items/1001"), false);
  const unique = dedupeProducts([
    { boothId: "1001", title: "先" },
    { boothId: "1001", title: "後" },
    { boothId: "1002", title: "次" },
  ]);
  assert.deepEqual(unique.map((product) => product.title), ["先", "次"]);
  assert.equal(quietHtml('<link rel="preload" as="script" href="https://example.test/a.js"><script src="https://example.test/a.js"></script><p>本文</p>').includes("script"), false);
});

test("a product missing from both the page and json is marked unpublished", () => {
  const product = { boothId: "1001", title: "消えた商品", shop: "工房", descriptionSnippet: "" };
  noteIfUnpublished(product, 404, 404);
  assert.equal(product.descriptionSnippet, UNAVAILABLE_TEXT);
  assert.equal(product.unavailable, "not-found");
  const kept = { boothId: "1002", title: "残っている", shop: "工房", descriptionSnippet: "説明があります" };
  noteIfUnpublished(kept, 404, 404);
  assert.equal(kept.descriptionSnippet, "説明があります");
  assert.equal(kept.unavailable, undefined);
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

test("update mode keeps newer purchases and stops at a known id", () => {
  const page = [
    { boothId: "9003", title: "新しい服" },
    { boothId: "9002", title: "その次" },
    { boothId: "9001", title: "インポート済み" },
    { boothId: "9000", title: "もっと古い" },
  ];
  const stopped = takeUntilKnown(page, new Set(["9001"]));
  assert.deepEqual(stopped.products.map((item) => item.boothId), ["9003", "9002"]);
  assert.equal(stopped.stopped, true);
  const open = takeUntilKnown(page, new Set(["8999"]));
  assert.equal(open.stopped, false);
  assert.equal(open.products.length, 4);
  const full = takeUntilKnown(page, null);
  assert.equal(full.stopped, false);
  assert.equal(full.products.length, 4);
});

test("item page records purchased variations and leaves the others unpurchased", () => {
  const html = `<!doctype html><body>
    <h2>架空の服</h2>
    <span class="price">無料</span>
    <span data-adult="true"></span>
    <div data-variation="通常">
      <span class="variation-name">通常</span>
      <a href="https://booth.pm/downloads/424242/file" download="body.unitypackage" data-size="12">body.unitypackage</a>
    </div>
    <div class="variation-item" data-owned="false">
      <span class="variation-name">特典</span>
    </div>
    <div class="item-card"><a href="/ja/items/9990001" download="other.unitypackage">other.unitypackage</a></div>
  </body>`;
  const { document } = parseHTML(html);
  const products = extractPage(document, "https://booth.pm/ja/items/424242");
  assert.equal(products.length, 1);
  assert.equal(products[0].isFree, true);
  assert.equal(products[0].isAdult, true);
  assert.deepEqual(
    products[0].variations.map((row) => [row.name, row.owned]),
    [["通常", true], ["特典", false]],
  );
  assert.deepEqual(products[0].downloads, [
    { filename: "body.unitypackage", sizeBytes: 12, variation: "通常" },
  ]);
});

test("library cards treat listed variations as purchased and keep download names", () => {
  const { document } = parseHTML(LIBRARY_HTML);
  const products = extractPage(document, "https://booth.pm/library");
  assert.deepEqual(products[0].downloads, [
    { filename: "manuka.zip", sizeBytes: null, variation: "" },
  ]);
  assert.equal(products[0].isFree, undefined);
  const html = `<!doctype html><body>
    <div class="bg-white">
      <img class="l-library-item-thumbnail" alt="" src="https://booth.pximg.net/c/72x72/a.jpg" />
      <a href="/ja/items/424242">架空の服</a>
      <span class="variation-name">通常</span>
      <span class="price">¥1,200</span>
    </div>
  </body>`;
  const priced = extractPage(parseHTML(html).document, "https://booth.pm/library");
  assert.deepEqual(priced[0].variations, [{ name: "通常", owned: true }]);
  assert.equal(priced[0].isFree, false);
});

test("library and gift pages keep downloadable ids and names beside the download link", () => {
  const html = `<!doctype html><body>
    <div class="mb-16 bg-white">
      <div class="gap-8 border-b pb-16">
        <a href="https://booth.pm/ja/items/424242">
          <img class="l-library-item-thumbnail" alt="" src="https://booth.pximg.net/c/72x72/a.jpg" />
        </a>
        <div>
          <a class="text-text-default" href="https://booth.pm/items/424242">架空の服</a>
          <div class="text-text-gray600">架空工房</div>
        </div>
      </div>
      <div class="mt-16">
        <div class="flex items-center justify-between">
          <div>
            <div>通常</div>
            <div class="flex min-w-0">
              <div class="truncate"><span>avatar_body</span></div>
              <div class="shrink-0">.unitypackage</div>
            </div>
          </div>
          <a href="https://booth.pm/downloadables/2336025">ダウンロード</a>
        </div>
        <div class="flex items-center justify-between">
          <div class="min-w-0" title="bonus_pack.zip">
            <div class="truncate">bonus_pack…</div>
          </div>
          <a href="https://booth.pm/downloadables/2336026"><span>ダウンロード</span></a>
        </div>
        <div class="flex items-center justify-between">
          <div>eye_texture.zip (12.4MB)</div>
          <a href="https://booth.pm/downloadables/2336027">ダウンロード</a>
        </div>
        <div class="flex items-center justify-between">
          <a href="https://booth.pm/downloadables/2336028">ダウンロード</a>
        </div>
      </div>
    </div>
  </body>`;
  const { document } = parseHTML(html);
  const library = extractPage(document, "https://accounts.booth.pm/library");
  const gifts = extractPage(document, "https://accounts.booth.pm/library/gifts");
  assert.equal(library.length, 1);
  assert.equal(gifts.length, 1);
  assert.deepEqual(library[0].downloads, [
    { filename: "avatar_body.unitypackage", sizeBytes: null, variation: "通常", downloadableId: "2336025" },
    { filename: "bonus_pack.zip", sizeBytes: null, variation: "", downloadableId: "2336026" },
    { filename: "eye_texture.zip", sizeBytes: null, variation: "", downloadableId: "2336027" },
    { filename: "2336028", sizeBytes: null, variation: "", downloadableId: "2336028" },
  ]);
  assert.deepEqual(library[0].variations, [{ name: "通常", owned: true }]);
  const report = diagnose(document, "https://accounts.booth.pm/library");
  const encoded = JSON.stringify(report);
  assert.equal(encoded.includes("架空の服"), false);
  assert.equal(encoded.includes("avatar_body"), false);
  assert.equal(encoded.includes("bonus_pack"), false);
  assert.equal(encoded.includes("eye_texture"), false);
  assert.equal(report.productCount, 1);
  assert.equal(report.variationCount, 1);
  assert.equal(report.filenameCount, 3);
  assert.equal(report.missingFilenameCount, 1);
  assert.ok(report.downloadLinkCount >= 4);
  assert.equal(report.selectorHits["img.l-library-item-thumbnail"], 1);
  assert.equal(report.selectorHits["a[href*='downloadables']"], 4);
  assert.equal(report.hrefShapes["https://booth.pm/downloadables/#"], 4);
});

test("item download rows use the title and nearby text, not the download label", () => {
  const html = `<!doctype html><body>
    <h2>架空の服</h2>
    <ul class="variations" id="variations">
      <li class="variation-item">
        <div>ダウンロード商品</div>
        <div class="variation-price">¥ 0</div>
        <a title="outfit_body.unitypackage" href="https://booth.pm/downloadables/2336025?variation_id=6076989">
          <span class="cmd-label">無料ダウンロード</span>
          <div>outfit_body</div>
          <div>.unitypackage</div>
        </a>
        <div class="js-download-free-button" data-dropdown-items='[{"text":"アプリでDL","deeplinkDownloadableUrl":"https://booth.pm/downloadables/2336025/deeplink?client=booth-library-manager&amp;variation_id=6076989","fallbackUrl":"https://booth.pm/announcements/1"}]'></div>
      </li>
    </ul>
  </body>`;
  const { document } = parseHTML(html);
  const products = extractPage(document, "https://booth.pm/ja/items/424242");
  assert.equal(products.length, 1);
  assert.equal(products[0].isFree, true);
  assert.deepEqual(products[0].downloads, [
    { filename: "outfit_body.unitypackage", sizeBytes: null, variation: "", downloadableId: "2336025" },
  ]);
  assert.deepEqual(products[0].variations, []);
  const report = diagnose(document, "https://booth.pm/ja/items/424242");
  const encoded = JSON.stringify(report);
  assert.equal(encoded.includes("架空の服"), false);
  assert.equal(encoded.includes("outfit_body"), false);
  assert.equal(report.hrefShapes["https://booth.pm/downloadables/#?variation_id=#"], 1);
  assert.equal(
    report.hrefShapes["https://booth.pm/downloadables/#/deeplink?client=#&variation_id=#"],
    1,
  );
});

test("item json fills a missing filename for the same downloadable id and records price flags", () => {
  const product = {
    boothId: "424242",
    title: "架空の服",
    shop: "",
    descriptionSnippet: "",
    downloads: [{ filename: "2336025", sizeBytes: null, variation: "", downloadableId: "2336025" }],
    variations: [],
  };
  const detail = detailFromItemJson({
    id: 424242,
    name: "架空の服",
    description: "説明",
    price: "¥ 0",
    is_adult: false,
    shop: { name: "架空工房" },
    variations: [{
      name: null,
      status: "free_download",
      price: 0,
      downloadable: {
        musics: [],
        no_musics: [{
          name: "outfit_body.unitypackage",
          file_name: "outfit_body",
          file_extension: ".unitypackage",
          url: "https://booth.pm/downloadables/2336025?variation_id=1",
        }],
      },
    }],
  }, "424242");
  mergeDetail(product, detail);
  assert.equal(product.isFree, true);
  assert.equal(product.isAdult, false);
  assert.equal(product.shop, "架空工房");
  assert.deepEqual(product.downloads, [
    { filename: "outfit_body.unitypackage", sizeBytes: null, variation: "", downloadableId: "2336025" },
  ]);
  const paid = detailFromItemJson({
    id: 1001,
    name: "有料の服",
    price: "¥ 6,000",
    is_adult: false,
    variations: [{ name: "特典", status: "addable_to_cart", downloadable: { no_musics: [], musics: [] } }],
  }, "1001");
  assert.equal(paid.isFree, false);
  assert.equal(paid.isAdult, false);
  assert.deepEqual(paid.downloads, []);
  assert.deepEqual(paid.variations, []);
});

test("card images use a pximg size the CDN serves", () => {
  assert.equal(
    cardImage("https://booth.pximg.net/abcdef/i/4876459/base.jpg"),
    "https://booth.pximg.net/c/150x150/abcdef/i/4876459/base.jpg",
  );
  assert.equal(
    cardImage("https://booth.pximg.net/c/72x72_a2_g5/abcdef/i/4876459/base.jpg"),
    "https://booth.pximg.net/c/150x150/abcdef/i/4876459/base.jpg",
  );
});
