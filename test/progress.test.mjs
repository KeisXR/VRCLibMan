import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

async function withModule(path, run) {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    await run(await server.ssrLoadModule(path));
  } finally {
    await server.close();
  }
}

const job = (name, current, total, extra = {}) => ({ job: name, current, total, message: `${name} ${current}/${total}`, ...extra });

test("legacy payloads without state keep the old end rule", async () => {
  await withModule("/src/progress.ts", (p) => {
    assert.equal(p.progressState(job("scan", 0, 0)), "running");
    assert.equal(p.progressState(job("scan", 1, 2)), "running");
    assert.equal(p.progressState(job("scan", 2, 2)), "done");
    assert.equal(p.progressState(job("scan", 2, 2, { state: "running" })), "running", "explicit state wins");
    assert.equal(p.progressState(job("scan", 0, 0, { state: "error" })), "error");
  });
});

test("per-job latest state, end handling and pruning", async () => {
  await withModule("/src/progress.ts", (p) => {
    let board = p.EMPTY_PROGRESS;
    board = p.applyProgress(board, job("thumbs", 0, 0), 0);
    assert.equal(p.workInProgress(board), true);
    // 0 件で終わる（total = 0 の done）も終わりとして扱う
    board = p.applyProgress(board, job("thumbs", 0, 0, { state: "done" }), 100);
    assert.equal(p.workInProgress(board), false);
    assert.equal(p.progressStrip(board).entry.state, "done");
    assert.equal(p.nextPruneDelay(board, 100), p.PROGRESS_LINGER_MS);
    assert.equal(p.pruneProgress(board, 100 + p.PROGRESS_LINGER_MS - 1), board, "unchanged before linger");
    board = p.pruneProgress(board, 100 + p.PROGRESS_LINGER_MS);
    assert.equal(p.progressStrip(board), null);
    assert.equal(p.nextPruneDelay(board, 0), null);

    // 不定のまま失敗しても消える
    board = p.applyProgress(board, job("catalog", 0, 0), 0);
    board = p.applyProgress(board, job("catalog", 0, 0, { state: "error", message: "中断しました" }), 10);
    assert.equal(p.workInProgress(board), false);
    board = p.pruneProgress(board, 10 + p.PROGRESS_LINGER_MS);
    assert.deepEqual(board.jobs, {});

    // 同じ仕事は最新で置き換える
    board = p.applyProgress(board, job("pull", 1, 10), 0);
    board = p.applyProgress(board, job("pull", 5, 10), 1);
    assert.equal(Object.keys(board.jobs).length, 1);
    assert.equal(board.jobs.pull.current, 5);
  });
});

test("top strip shows the latest foreground job and counts other running ones", async () => {
  await withModule("/src/progress.ts", (p) => {
    let board = p.EMPTY_PROGRESS;
    board = p.applyProgress(board, job("scan", 0, 0), 0);
    board = p.applyProgress(board, job("thumbs", 1, 4), 1);
    let strip = p.progressStrip(board);
    assert.equal(strip.entry.job, "thumbs");
    assert.equal(strip.others, 1);
    assert.equal(p.othersLabel(strip.others), "ほか 1 件");
    assert.equal(p.othersLabel(0), "");

    board = p.applyProgress(board, job("scan", 0, 0), 2);
    strip = p.progressStrip(board);
    assert.equal(strip.entry.job, "scan", "most recently updated wins");

    // 終わった仕事は「ほか」に数えない
    board = p.applyProgress(board, job("thumbs", 4, 4, { state: "done" }), 3);
    strip = p.progressStrip(board);
    assert.equal(strip.entry.job, "thumbs");
    assert.equal(strip.others, 1);
    board = p.applyProgress(board, job("scan", 9, 9, { state: "done" }), 4);
    assert.equal(p.progressStrip(board).others, 0);
  });
});

test("background jobs never reach the top strip or workInProgress", async () => {
  await withModule("/src/progress.ts", (p) => {
    let board = p.EMPTY_PROGRESS;
    board = p.applyProgress(board, job("asset-updates", 1, 9, { background: true }), 0);
    assert.equal(p.progressStrip(board), null);
    assert.equal(p.workInProgress(board), false);
    board = p.applyProgress(board, job("copy", 0, 1), 1);
    board = p.applyProgress(board, job("asset-updates", 2, 9, { background: true }), 2);
    const strip = p.progressStrip(board);
    assert.equal(strip.entry.job, "copy");
    assert.equal(strip.others, 0);
    assert.equal(p.workInProgress(board), true);
  });
});

test("progress percent: determinate, indeterminate and ended", async () => {
  await withModule("/src/progress.ts", (p) => {
    assert.equal(p.progressPercent(1, 4), 25);
    assert.equal(p.progressPercent(9, 4), 100);
    assert.equal(p.progressPercent(0, 0), null);
    assert.equal(p.progressPercent(0, 0, "done"), 100);
    assert.equal(p.progressPercent(0, 0, "error"), 0, "ended indeterminate stops animating");
    assert.equal(p.progressPercent(2, 4, "cancelled"), 50);
  });
});

test("ProgressTrack keeps the .track markup and exposes progressbar semantics", async () => {
  const { createElement } = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  await withModule("/src/ui.tsx", (ui) => {
    const determinate = renderToStaticMarkup(createElement(ui.ProgressTrack, { current: 1, total: 4, label: "1 / 4" }));
    assert.match(determinate, /^<div class="track" role="progressbar"/);
    assert.match(determinate, /aria-valuenow="25"/);
    assert.match(determinate, /aria-label="1 \/ 4"/);
    assert.match(determinate, /<span style="width:25%"><\/span><\/div>$/);
    const indeterminate = renderToStaticMarkup(createElement(ui.ProgressTrack, { current: 0, total: 0, label: "調べています" }));
    assert.match(indeterminate, /^<div class="track indeterminate" role="progressbar"/);
    assert.doesNotMatch(indeterminate, /aria-valuenow/);
    assert.match(indeterminate, /<span><\/span><\/div>$/);
  });
});

test("screens use the shared ProgressTrack instead of duplicated markup", () => {
  for (const file of ["src/components/ProgressStripBar.tsx"]) {
    const source = readFileSync(file, "utf8");
    assert.match(source, /<ProgressTrack/, file);
    assert.doesNotMatch(source, /track indeterminate/, file);
  }
});

test("Rust progress uses the shared helper and guards the never-clearing jobs", () => {
  const commands = readFileSync("src-tauri/src/commands.rs", "utf8");
  const browser = readFileSync("src-tauri/src/browser.rs", "utf8");
  assert.doesNotMatch(browser, /"job-progress"/, "browser.rs emits through progress.rs");
  assert.doesNotMatch(commands, /"job-progress"/, "commands.rs emits through progress.rs");
  for (const name of ["scan", "thumbs", "catalog", "copy"]) {
    assert.match(commands, new RegExp(`ProgressGuard::new\\(app\\.clone\\(\\), "${name}"\\)`), name);
  }
  for (const name of ["browser-library"]) {
    assert.match(browser, new RegExp(`ProgressGuard::new\\(app\\.clone\\(\\), "${name}"\\)`), name);
  }
  // BOOTH からの受け取りは上部の帯ではなく、サイドバーの「ダウンロード」に booth-file-download で出す。
  assert.doesNotMatch(browser, /ProgressGuard::new\(app\.clone\(\), "download"\)/);
  assert.match(browser, /FileReport::new\(app\.clone\(\)/);
});

test("progress wording rule: count first, name allowed, one line with ellipsis", () => {
  const css = readFileSync("src/styles.css", "utf8");
  const head = css.match(/\.progress-head p \{([^}]*)\}/)?.[1] ?? "";
  for (const decl of ["text-overflow: ellipsis", "white-space: nowrap", "overflow: hidden", "min-width: 0"]) {
    assert.ok(head.includes(decl), `.progress-head p needs ${decl}`);
  }
  const docs = readFileSync("docs/design/README.md", "utf8");
  assert.ok(!docs.includes("文言は件数だけ"), "old count-only rule removed");
  assert.ok(docs.includes("分類中 3 / 10: 商品名"));
  const agents = readFileSync("AGENTS.md", "utf8");
  assert.ok(!agents.includes("文言は件数だけ"));
  const rs = readFileSync("src-tauri/src/commands.rs", "utf8");
  assert.ok(!rs.includes("分類中: "), "extract message carries the count");
  assert.ok(!rs.includes("をコピーしています"), "copy message carries the count");
});

test("the top strip slides out with its last message, and goes at once when motion is reduced", async () => {
  const { parseHTML } = await import("linkedom");
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  for (const name of ["window", "document", "HTMLElement", "Element", "Node", "Event", "EventTarget"]) {
    if (window[name] !== undefined) Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const timers = new Map();
  let nextTimer = 1;
  window.setTimeout = (fn) => { const id = nextTimer++; timers.set(id, fn); return id; };
  window.clearTimeout = (id) => { timers.delete(id); };
  let reduced = false;
  window.matchMedia = (query) => ({ matches: query.includes("reduce") && reduced, media: query });
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { ProgressStripBar } = await server.ssrLoadModule("/src/components/ProgressStripBar.tsx");
    const entry = (message) => ({ entry: { id: "job", message, current: 1, total: 2, state: "running", seq: 1 }, others: 0 });
    root = createRoot(document.getElementById("root"));
    const render = (strip) => act(async () => { root.render(createElement(ProgressStripBar, { strip })); });
    const strip = () => document.querySelector(".progress");
    const fire = (name) => act(async () => {
      const event = new window.Event("animationend", { bubbles: true });
      event.animationName = name;
      strip().dispatchEvent(event);
    });

    await render(entry("分類中 1 / 2"));
    assert.equal(strip().getAttribute("data-topbar-motion"), "");
    assert.equal(strip().getAttribute("data-leaving"), null);
    await render(null);
    assert.ok(strip() !== null, "the strip stays while it slides out");
    assert.equal(strip().getAttribute("data-leaving"), "true");
    assert.ok(strip().textContent.includes("分類中 1 / 2"), "it keeps the last message");
    // 閉じる途中で仕事が戻ったら、作り直さずにそのまま出し直す。
    const same = strip();
    await render(entry("分類中 2 / 2"));
    assert.ok(strip() === same, "the same element comes back");
    assert.equal(strip().getAttribute("data-leaving"), null);
    await render(null);
    await fire("topbar-fade-out");
    assert.ok(strip() !== null, "only the collapse ends the leave");
    await fire("topbar-collapse");
    assert.ok(strip() === null, "removed after the collapse");

    // animationend が来なくても上限の時間で取り除く。
    await render(entry("読み込み中"));
    await render(null);
    assert.ok(strip() !== null);
    await act(async () => { for (const fn of [...timers.values()]) fn(); });
    assert.ok(strip() === null, "the fallback removes the strip");

    reduced = true;
    await render(entry("読み込み中"));
    await render(null);
    assert.ok(strip() === null, "reduced motion removes the strip at once");
  } finally {
    if (root) await act(async () => root.unmount());
    delete window.matchMedia;
    await server.close();
  }
});

test("progress bars glide instead of stepping", () => {
  const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  assert.match(css, /\.track span \{[^}]*transition: width var\(--motion-rail\) var\(--ease-out\);/);
});
