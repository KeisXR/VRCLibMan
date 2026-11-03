// 線のアクセント（朱）の決まりを styles.css で確かめる。
// 決まり: 曲線（角丸の辺に沿って曲がる線）にアクセントを付けない。許すのは全周の枠、
// 直線の縦棒（::before）、直線の下線（::after）だけ。どちらも端の丸みは 2px まで（例外）。
// 進捗バー（.track span）は例外。詳細は docs/design/README.md「線のアクセント」。
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
const tokens = readFileSync(new URL("../docs/design/tokens.css", import.meta.url), "utf8");

const START = "==== 線のアクセント（朱）: 指定はこのブロックだけ ====";
const END = "==== 線のアクセントここまで ====";
const ACCENT = /var\(--line-(?:accent|selection)\)/;

// 朱で面を塗ってよいもの（例外）。
const FILL_ALLOWLIST = [".track span"];

/** コメントを同じ長さの空白に置き換え、位置を保ったまま規則を拾う。 */
function parseRules(source) {
  const text = source.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
  const rules = [];
  const stack = [];
  let segStart = 0;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (ch === "{") {
      stack.push({ selector: text.slice(segStart, i).trim(), open: i, nested: false });
      if (stack.length > 1) stack[stack.length - 2].nested = true;
      segStart = i + 1;
    } else if (ch === "}") {
      const top = stack.pop();
      if (top && !top.nested && !top.selector.startsWith("@")) {
        const body = text.slice(top.open + 1, i);
        const decls = body
          .split(";")
          .map((d) => d.trim())
          .filter(Boolean)
          .map((d) => {
            const at = d.indexOf(":");
            return { prop: d.slice(0, at).trim(), value: d.slice(at + 1).trim() };
          });
        rules.push({ selector: top.selector, selectors: top.selector.split(",").map((s) => s.trim()), decls, start: top.open });
      }
      segStart = i + 1;
    } else if (ch === ";" && stack.length === 0) {
      segStart = i + 1;
    }
  }
  return rules;
}

const blockStart = css.indexOf(START);
const blockEnd = css.indexOf(END);
const rules = parseRules(css);
const inBlock = (rule) => rule.start > blockStart && rule.start < blockEnd;
const accentDecls = rules.flatMap((rule) =>
  rule.decls.filter((d) => ACCENT.test(d.value) || /^--line-(?:accent|selection)$/.test(d.prop)).map((d) => ({ rule, ...d })),
);

/** box-shadow の値を、括弧の外のカンマで分ける。 */
function splitShadows(value) {
  const parts = [];
  let depth = 0;
  let from = 0;
  for (let i = 0; i < value.length; i += 1) {
    if (value[i] === "(") depth += 1;
    else if (value[i] === ")") depth -= 1;
    else if (value[i] === "," && depth === 0) {
      parts.push(value.slice(from, i).trim());
      from = i + 1;
    }
  }
  parts.push(value.slice(from).trim());
  return parts;
}

test("accent block exists once and the rule is stated in it", () => {
  assert.ok(blockStart >= 0 && blockEnd > blockStart, "styles.css に線のアクセントの集約ブロックがある");
  assert.equal(css.indexOf(START, blockStart + 1), -1, "集約ブロックは1つだけ");
  assert.match(css.slice(blockStart, blockEnd), /曲線/);
  assert.ok(accentDecls.length > 0);
});

test("(a) --line-accent / --line-selection are only defined and used inside the accent block", () => {
  const outside = accentDecls.filter((d) => !inBlock(d.rule));
  assert.deepEqual(
    outside.map((d) => `${d.rule.selector} { ${d.prop}: ${d.value} }`),
    [],
    "朱の指定は styles.css の「線のアクセント」ブロックに集約する",
  );
});

test("(b) the accent is never a one-sided inset shadow (only a full ring)", () => {
  for (const d of accentDecls.filter((x) => x.prop === "box-shadow")) {
    for (const shadow of splitShadows(d.value).filter((s) => ACCENT.test(s))) {
      const lengths = shadow
        .replace(ACCENT, "")
        .replace(/\binset\b/, "")
        .trim()
        .split(/\s+/)
        .filter(Boolean);
      const [x = "0", y = "0"] = lengths;
      assert.ok(
        parseFloat(x) === 0 && parseFloat(y) === 0,
        `${d.rule.selector}: 朱の影は全周（オフセット 0）だけ。片側の線は ::after の直線の下線にする: ${shadow}`,
      );
    }
  }
});

test("(c) the accent is never a one-sided border", () => {
  const sided = accentDecls.filter((d) => /^border-(?:left|right|top|bottom|inline|block)/.test(d.prop));
  assert.deepEqual(
    sided.map((d) => `${d.rule.selector} { ${d.prop}: ${d.value} }`),
    [],
    "角丸の要素の片側だけに朱の線を付けると角で曲がる",
  );
});

/** 縦棒・下線の端の丸みが 2px 以内か。var(--accent-cap) は定義の値で見る。 */
function smallCap(radius) {
  if (radius === undefined) return true;
  const resolved = radius === "var(--accent-cap)" ? css.match(/--accent-cap:\s*([^;]+);/)?.[1].trim() : radius;
  return /^(?:0|(?:[0-1](?:\.\d+)?|2(?:\.0+)?)px)$/.test(resolved ?? "");
}

test("(d) accent fills are straight lines whose ends are rounded at most 2px", () => {
  const fills = accentDecls.filter((d) => d.prop === "background" || d.prop === "background-color");
  for (const d of fills) {
    const radius = d.rule.decls.find((x) => x.prop === "border-radius")?.value;
    if (d.rule.selectors.every((s) => FILL_ALLOWLIST.includes(s))) continue; // 進捗バー（例外）
    if (d.rule.selectors.every((s) => s.endsWith("::after"))) {
      assert.ok(smallCap(radius), `${d.rule.selector}: 下線の端の丸みは縦棒と同じ 2px まで: ${radius}`);
      for (const side of ["left", "right"]) {
        const v = d.rule.decls.find((x) => x.prop === side)?.value;
        if (v !== undefined) assert.match(v, /--accent-inset/, `${d.rule.selector}: 下線は角丸の内側（--accent-inset）から引く`);
      }
    } else if (d.rule.selectors.every((s) => s.endsWith("::before"))) {
      // 縦棒の端の小さな丸みは曲線に含めない（例外）。2px を超える丸みは認めない。
      assert.ok(smallCap(radius), `${d.rule.selector}: 縦棒の端の丸みは 2px まで: ${radius}`);
    } else {
      assert.fail(`${d.rule.selector}: 朱で面を塗らない（直線の縦棒・下線の疑似要素か、進捗バーだけ）`);
    }
  }
  assert.match(css, /--accent-inset:\s*var\(--radius-sm\)/, "下線の左右の余白はタブの角丸と同じ");
});

test("tab underline is the straight ::after line, not a curved shadow", () => {
  const active = rules.filter((r) => r.selectors.includes(".tab.active"));
  for (const r of active) {
    for (const d of r.decls.filter((x) => x.prop === "box-shadow")) assert.doesNotMatch(d.value, ACCENT);
  }
  assert.ok(rules.some((r) => r.selectors.includes(".tab.active::after") && inBlock(r)));
});

test("tokens.css carries the same accent variables", () => {
  for (const name of ["--line-accent", "--line-selection", "--selection-outline", "--accent-line", "--accent-bar", "--accent-bar-lg", "--accent-inset", "--accent-offset", "--accent-cap"]) {
    const value = (source) => source.match(new RegExp(`${name}:\\s*([^;]+);`))?.[1].trim();
    assert.ok(value(tokens), `tokens.css に ${name} がある`);
    assert.equal(value(tokens), value(css), `${name} の値が styles.css と同じ`);
  }
});
