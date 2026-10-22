import { cpSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";

const denyNames = new Set([
  "package.json",
  "package-lock.json",
  "pnpm-lock.yaml",
  "yarn.lock",
]);

const args = process.argv.slice(2);
let source = "extension";
let dest = "";
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === "--source") {
    source = args[++i];
  } else if (args[i] === "--dest") {
    dest = args[++i];
  } else {
    console.error(`unknown argument ${args[i]}`);
    process.exit(1);
  }
}
if (!dest) {
  console.error("usage: node scripts/stage-extension.mjs --dest <dir> [--source extension]");
  process.exit(1);
}

const sourceRoot = resolve(source);
const destRoot = resolve(dest);

function repoPath(absPath) {
  return relative(sourceRoot, absPath).split(sep).join("/");
}

function isDenied(relPath) {
  const parts = relPath.split("/");
  return parts.includes("node_modules") || denyNames.has(parts.at(-1));
}

rmSync(destRoot, { recursive: true, force: true });
mkdirSync(destRoot, { recursive: true });
cpSync(sourceRoot, destRoot, {
  recursive: true,
  filter: (src) => resolve(src) === sourceRoot || !isDenied(repoPath(src)),
});

function listFiles(dir) {
  const found = [];
  const walk = (current) => {
    for (const entry of readdirSync(current)) {
      const abs = join(current, entry);
      if (statSync(abs).isDirectory()) walk(abs);
      else found.push(relative(dir, abs).split(sep).join("/"));
    }
  };
  walk(dir);
  return found.sort();
}

const staged = new Set(listFiles(destRoot));
const missing = [];

function requireFile(relPath, from) {
  const normalized = relPath.split(sep).join("/");
  if (!normalized || normalized.startsWith("/") || normalized.includes("..")) {
    missing.push(`${normalized} (from ${from})`);
    return;
  }
  if (!staged.has(normalized)) missing.push(`${normalized} (from ${from})`);
}

function pushStrings(value, from, refs) {
  if (typeof value === "string") refs.push({ path: value, from });
  else if (Array.isArray(value)) {
    for (const item of value) pushStrings(item, from, refs);
  } else if (value && typeof value === "object") {
    for (const item of Object.values(value)) pushStrings(item, from, refs);
  }
}

const manifestPath = join(sourceRoot, "manifest.json");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const refs = [];
for (const script of manifest.content_scripts ?? []) {
  pushStrings(script.js, "manifest.json content_scripts.js", refs);
  pushStrings(script.css, "manifest.json content_scripts.css", refs);
}
pushStrings(manifest.action?.default_popup, "manifest.json action.default_popup", refs);
pushStrings(manifest.action?.default_icon, "manifest.json action.default_icon", refs);
pushStrings(manifest.icons, "manifest.json icons", refs);
pushStrings(manifest.background?.service_worker, "manifest.json background.service_worker", refs);
pushStrings(manifest.background?.scripts, "manifest.json background.scripts", refs);
pushStrings(manifest.options_page, "manifest.json options_page", refs);
pushStrings(manifest.options_ui?.page, "manifest.json options_ui.page", refs);
pushStrings(manifest.side_panel?.default_path, "manifest.json side_panel.default_path", refs);
pushStrings(manifest.devtools_page, "manifest.json devtools_page", refs);
pushStrings(manifest.sandbox?.pages, "manifest.json sandbox.pages", refs);
pushStrings(manifest.chrome_url_overrides, "manifest.json chrome_url_overrides", refs);
for (const group of manifest.web_accessible_resources ?? []) {
  pushStrings(group.resources, "manifest.json web_accessible_resources", refs);
}

function globToRegExp(glob) {
  let body = "^";
  for (const char of glob) {
    if (char === "*") body += ".*";
    else if (char === "?") body += ".";
    else if ("\\^$+?.()|[]{}".includes(char)) body += `\\${char}`;
    else body += char;
  }
  return new RegExp(`${body}$`);
}

function requireRef(ref) {
  if (ref.path.includes("*") || ref.path.includes("?")) {
    const pattern = globToRegExp(ref.path);
    if (![...staged].some((file) => pattern.test(file))) {
      missing.push(`${ref.path} (from ${ref.from})`);
    }
    return;
  }
  requireFile(ref.path, ref.from);
}

for (const ref of refs) requireRef(ref);

const localUrl = /(?:src|href)\s*=\s*["']([^"']+)["']/gi;
for (const file of staged) {
  if (!file.endsWith(".html")) continue;
  const html = readFileSync(join(destRoot, file), "utf8");
  for (const match of html.matchAll(localUrl)) {
    const url = match[1].trim();
    if (!url || /^(?:[a-z]+:|\/\/|#|data:)/i.test(url)) continue;
    const clean = url.split(/[?#]/, 1)[0];
    const abs = resolve(destRoot, dirname(file), clean);
    const rel = relative(destRoot, abs).split(sep).join("/");
    requireFile(rel, file);
  }
}

if (!staged.has("manifest.json")) missing.push("manifest.json (extension root)");

if (missing.length > 0) {
  console.error("extension zip is missing files:");
  for (const item of missing) console.error(`- ${item}`);
  process.exit(1);
}

console.log([...staged].join("\n"));
