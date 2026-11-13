// 切替の動き（styles.css の「切替の動き」ブロック）の決まりを確かめる。
// 決まり: 動かすのは opacity と transform だけ。長さは 120〜200ms（--motion、左サイドバーと同じ --motion-fast / --motion-rail も可）。
// prefers-reduced-motion では全体の animation / transition を止める。詳細は docs/design/README.md「動き」。
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
const START = "==== 切替の動き:";
const END = "==== 切替の動きここまで ====";

function block() {
  const s = css.indexOf(START);
  const e = css.indexOf(END);
  assert.ok(s >= 0 && e > s, "切替の動きブロックがある");
  return css.slice(s, e).replace(/\/\*[\s\S]*?\*\//g, "");
}

test("keyframes は opacity と transform だけを動かす", () => {
  const body = block();
  const frames = [...body.matchAll(/@keyframes\s+([\w-]+)\s*\{([\s\S]*?)\}\s*\}/g)];
  assert.ok(frames.length >= 3);
  for (const [, name, inner] of frames) {
    const props = [...inner.matchAll(/([a-z-]+)\s*:/g)].map((m) => m[1]);
    for (const prop of props) assert.ok(["opacity", "transform"].includes(prop), `${name} が ${prop} を動かしている`);
  }
});

test("animation の長さは --motion・左サイドバーと同じ変数か 120〜200ms", () => {
  for (const [, value] of block().matchAll(/animation:\s*([^;]+);/g)) {
    const ms = value.match(/(\d+)ms/);
    if (ms) {
      const n = Number(ms[1]);
      assert.ok(n >= 120 && n <= 200, `${value} の長さ`);
    } else {
      assert.match(value, /var\(--motion(-fast|-rail)?\)/, `${value} の長さ`);
    }
  }
  assert.match(css, /--motion:\s*160ms/);
});

test("transition は地・文字・影・不透明度・transform だけ", () => {
  for (const [, value] of block().matchAll(/transition:\s*([^;]+);/g)) {
    for (const part of value.split(",")) {
      const prop = part.trim().split(/\s+/)[0];
      assert.ok(["background-color", "color", "box-shadow", "opacity", "transform"].includes(prop), `transition ${prop}`);
    }
  }
});

test("右パネルとその親には animation を付けない", () => {
  for (const [, selector] of block().matchAll(/([^{}]+)\{[^{}]*animation/g)) {
    assert.doesNotMatch(selector, /right-panel(?!-popover)|\.app\b|\.content\b|\.workspace/, selector.trim());
  }
});

test("prefers-reduced-motion でグローバルに止める", () => {
  assert.match(
    css,
    /@media \(prefers-reduced-motion: reduce\)\s*\{\s*\*, \*::before, \*::after\s*\{\s*animation: none !important;\s*transition: none !important;/,
  );
});
