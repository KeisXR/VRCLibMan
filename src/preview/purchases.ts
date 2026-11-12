// 見本の購入履歴。分類の決め方、検索、ページ分け、集計は Rust（purchases.rs）の複製で、
// Rust を直しても自動では追従しない。注文番号、商品、金額はすべて架空のもの。
import type {
  JobProgress,
  PurchaseGroup,
  PurchaseItemView,
  PurchaseLabel,
  PurchaseOrderView,
  PurchasePage,
  PurchaseQuery,
  PurchaseStatus,
  PurchaseSummary,
  PurchaseSummaryGroup,
  PurchaseWalkReport,
} from "../types";

export interface PreviewPurchaseContext {
  builtinBrowser: boolean;
  emitProgress: (progress: JobProgress) => void;
}

const GIFT_LABEL = "ギフト（贈った）";
const PAGE_SIZE = 50;
const bus = new EventTarget();

export function listenPreviewPurchases(onChange: () => void): () => void {
  const listener = () => onChange();
  bus.addEventListener("change", listener);
  return () => bus.removeEventListener("change", listener);
}

function changed(): void {
  bus.dispatchEvent(new Event("change"));
}

const avatar = (id: string, label: string, body = false): PurchaseGroup => ({ key: `avatar:${id}`, kind: "avatar", label, avatarId: id, body });
const GENERIC: PurchaseGroup = { key: "generic", kind: "generic", label: "汎用", avatarId: null, body: false };
const WORLD: PurchaseGroup = { key: "world", kind: "world", label: "ワールド", avatarId: null, body: false };
const UNCLASSIFIED: PurchaseGroup = { key: "unclassified", kind: "unclassified", label: "分類なし", avatarId: null, body: false };
const OWNED = new Set(["lime", "plum", "manuka"]);

interface Sample {
  boothId: string;
  name: string;
  variation: string | null;
  shop: string;
  priceYen: number | null;
  boostYen: number | null;
  inLibrary: boolean;
  thumbnailUrl: string | null;
  groups: PurchaseGroup[];
  /** 見本のライブラリ（preview/api.ts）にある商品だけ。 */
  libraryLink?: PurchaseItemView["libraryLink"];
}

const POOL: Sample[] = [
  { boothId: "8800101", name: "しなの・ライム・マヌカ対応 ワンピース (フルセット)", variation: "フルセット", shop: "サンプル工房", priceYen: 2500, boostYen: 300, inLibrary: true, thumbnailUrl: "/samples/ribbon.svg", groups: [{ key: "multiple", kind: "multiple", label: "複数アバター対応", avatarId: null, body: false }], libraryLink: { kind: "asset", avatarId: null } },
  { boothId: "8800140", name: "見本のヘアピン", variation: null, shop: "見本工房", priceYen: 300, boostYen: 0, inLibrary: true, thumbnailUrl: null, groups: [GENERIC] },
  { boothId: "8800141", name: "プラム用 見本テクスチャ (プラム版)", variation: "プラム版", shop: "見本工房", priceYen: 800, boostYen: 0, inLibrary: true, thumbnailUrl: null, groups: [avatar("plum", "プラム")] },
  { boothId: "8800142", name: "見本のワールド小物", variation: null, shop: "見本ワールド店", priceYen: 500, boostYen: 0, inLibrary: true, thumbnailUrl: null, groups: [WORLD] },
  { boothId: "8800143", name: "マヌカ向け 見本パーカー (マヌカ版)", variation: "マヌカ版", shop: "サンプル工房", priceYen: 1800, boostYen: 200, inLibrary: true, thumbnailUrl: "/samples/seal.svg", groups: [avatar("manuka", "マヌカ")] },
  { boothId: "4876459", name: "ライム -Lime- / オリジナル3Dモデル", variation: null, shop: "こまど", priceYen: 5500, boostYen: 0, inLibrary: true, thumbnailUrl: "/samples/avatar-hero.png", groups: [avatar("lime", "ライム", true)], libraryLink: { kind: "avatar", avatarId: "lime" } },
  { boothId: "8800144", name: "ライム・プラム対応 見本の髪", variation: null, shop: "見本ヘア", priceYen: 1200, boostYen: 0, inLibrary: true, thumbnailUrl: null, groups: [{ key: "unset", kind: "unset", label: "対応版未設定", avatarId: null, body: false }] },
  { boothId: "8800145", name: "見本のグッズ", variation: null, shop: "見本工房", priceYen: 400, boostYen: 0, inLibrary: false, thumbnailUrl: null, groups: [UNCLASSIFIED] },
];

interface StoredItem {
  id: number;
  sample: Sample;
  labelIds: number[];
}

interface StoredOrder {
  orderId: string;
  orderedAt: string | null;
  recordedAt: string;
  totalYen: number | null;
  giftDetected: boolean;
  giftOverride: boolean | null;
  detailRead: boolean;
  source: string;
  title: string;
  items: StoredItem[];
}

const state = {
  complete: false,
  lastReadAt: null as string | null,
  showPrices: false,
  running: false,
  cancel: false,
  nextLabel: 1,
  labels: [] as { id: number; name: string }[],
  orders: [] as StoredOrder[],
};

function jst(time: number): string {
  return `${new Date(time + 9 * 3600_000).toISOString().slice(0, 19)}+09:00`;
}

function buildOrders(): StoredOrder[] {
  const orders: StoredOrder[] = [];
  let id = 1;
  const start = Date.UTC(2026, 10, 2, 0, 30, 20);
  orders.push({
    orderId: "90300001",
    orderedAt: null,
    recordedAt: jst(Date.UTC(2026, 10, 10, 13, 15, 0)),
    totalYen: null,
    giftDetected: false,
    giftOverride: null,
    detailRead: false,
    source: "checkout",
    title: POOL[6].name,
    items: [{ id: id++, sample: { ...POOL[6], priceYen: null, boostYen: null }, labelIds: [] }],
  });
  for (let index = 0; index < 64; index += 1) {
    const at = start - index * 9 * 86_400_000 - index * 3_671_000;
    const samples = [POOL[index % POOL.length]];
    if (index % 7 === 3) samples.push(POOL[(index + 3) % POOL.length]);
    orders.push({
      orderId: String(90221765 - index * 7919),
      orderedAt: jst(at),
      recordedAt: jst(at),
      totalYen: samples.reduce((sum, sample) => sum + (sample.priceYen ?? 0) + (sample.boostYen ?? 0), 0),
      giftDetected: index === 5,
      giftOverride: null,
      detailRead: true,
      source: "history",
      title: samples[0].name,
      items: samples.map((sample) => ({ id: id++, sample, labelIds: [] })),
    });
  }
  return orders;
}

function gift(order: StoredOrder): boolean {
  return order.giftOverride ?? order.giftDetected;
}

function itemView(item: StoredItem): PurchaseItemView {
  const sample = item.sample;
  return {
    id: item.id,
    boothId: sample.boothId,
    name: sample.variation ? sample.name.replace(` (${sample.variation})`, "") : sample.name,
    variation: sample.variation,
    shop: sample.shop,
    priceYen: sample.priceYen,
    boostYen: sample.boostYen,
    amountYen: sample.priceYen == null ? null : sample.priceYen + (sample.boostYen ?? 0),
    quantity: 1,
    inLibrary: sample.inLibrary,
    libraryLink: sample.libraryLink ?? null,
    thumbnailPath: null,
    thumbnailUrl: sample.thumbnailUrl,
    groups: sample.groups,
    labelIds: [...item.labelIds],
  };
}

function orderView(order: StoredOrder): PurchaseOrderView {
  return {
    orderId: order.orderId,
    orderedAt: order.orderedAt,
    recordedAt: order.recordedAt,
    totalYen: order.totalYen,
    gift: gift(order),
    giftDetected: order.giftDetected,
    giftOverride: order.giftOverride,
    detailRead: order.detailRead,
    source: order.source,
    title: order.title,
    items: order.items.map(itemView),
  };
}

function yearOf(order: StoredOrder): number {
  return Number((order.orderedAt ?? order.recordedAt).slice(0, 4));
}

function labels(): PurchaseLabel[] {
  return state.labels
    .map((label) => ({
      ...label,
      items: state.orders.reduce((sum, order) => sum + order.items.filter((item) => item.labelIds.includes(label.id)).length, 0),
    }))
    .sort((a, b) => a.name.localeCompare(b.name, "ja"));
}

function status(): PurchaseStatus {
  return {
    orders: state.orders.length,
    items: state.orders.reduce((sum, order) => sum + order.items.length, 0),
    pendingDetails: state.orders.filter((order) => !order.detailRead).length,
    complete: state.complete,
    lastReadAt: state.lastReadAt,
    showPrices: state.showPrices,
    running: state.running,
  };
}

function normalize(text: string): string {
  return text.normalize("NFKC").toLowerCase();
}

function page(query: PurchaseQuery): PurchasePage {
  const names = new Map(state.labels.map((label) => [label.id, label.name]));
  const tokens = normalize(query.query ?? "").split(/\s+/).filter(Boolean);
  const matched = state.orders.filter((order) => {
    const view = orderView(order);
    if (query.gift != null && view.gift !== query.gift) return false;
    if (query.year != null && yearOf(order) !== query.year) return false;
    if (query.labelId != null && !view.items.some((item) => item.labelIds.includes(query.labelId as number))) return false;
    if (query.group && !view.items.some((item) => item.groups.some((group) => group.key === query.group))) return false;
    if (tokens.length === 0) return true;
    const text = normalize(
      [view.orderId, view.title, view.gift ? GIFT_LABEL : "", ...view.items.flatMap((item) => [item.name, item.shop, item.boothId, ...item.groups.map((group) => group.label), ...item.labelIds.map((id) => names.get(id) ?? "")])].join("\n"),
    );
    return tokens.every((token) => text.includes(token));
  });
  const groups = new Map<string, { key: string; kind: PurchaseGroup["kind"]; label: string; items: number }>();
  for (const order of state.orders) {
    for (const item of order.items) {
      for (const group of item.sample.groups) {
        const entry = groups.get(group.key) ?? { key: group.key, kind: group.kind, label: group.label, items: 0 };
        entry.items += 1;
        groups.set(group.key, entry);
      }
    }
  }
  const rank = (kind: string) => (kind === "avatar" ? 0 : kind === "generic" ? 1 : 2);
  const limit = Math.min(200, Math.max(1, query.limit ?? PAGE_SIZE));
  const total = matched.length;
  const requested = query.offset ?? 0;
  const offset = requested >= total ? Math.floor(Math.max(0, total - 1) / limit) * limit : requested - (requested % limit);
  return {
    orders: matched.slice(offset, offset + limit).map(orderView),
    total,
    offset,
    limit,
    labels: labels(),
    groups: [...groups.values()].sort((a, b) => rank(a.kind) - rank(b.kind) || a.label.localeCompare(b.label, "ja")),
    years: [...new Set(state.orders.map(yearOf))].sort((a, b) => b - a),
    status: status(),
  };
}

function summary(year: number | null): PurchaseSummary {
  const result: PurchaseSummary = {
    year,
    years: [...new Set(state.orders.map(yearOf))].sort((a, b) => b - a),
    orders: 0,
    items: 0,
    totalYen: 0,
    unknownAmountItems: 0,
    giftItems: 0,
    giftYen: 0,
    groups: [],
    showPrices: state.showPrices,
  };
  const groups = new Map<string, PurchaseSummaryGroup>();
  for (const order of state.orders.filter((candidate) => year == null || yearOf(candidate) === year)) {
    result.orders += 1;
    for (const item of order.items.map(itemView)) {
      result.items += 1;
      if (item.amountYen == null) result.unknownAmountItems += 1;
      else result.totalYen += item.amountYen;
      if (gift(order)) {
        result.giftItems += 1;
        result.giftYen += item.amountYen ?? 0;
        continue;
      }
      for (const group of item.groups) {
        const entry = groups.get(group.key) ?? {
          key: group.key,
          kind: group.kind,
          label: group.label,
          avatarId: group.avatarId,
          owned: group.avatarId != null && OWNED.has(group.avatarId),
          items: 0,
          bodyItems: 0,
          amountYen: 0,
          unknownAmountItems: 0,
        };
        entry.items += 1;
        if (group.body) entry.bodyItems += 1;
        if (item.amountYen == null) entry.unknownAmountItems += 1;
        else entry.amountYen += item.amountYen;
        groups.set(group.key, entry);
      }
    }
  }
  const rank = (kind: string) => (kind === "avatar" ? 0 : kind === "generic" ? 1 : 2);
  result.groups = [...groups.values()].sort((a, b) => rank(a.kind) - rank(b.kind) || b.amountYen - a.amountYen || b.items - a.items || a.label.localeCompare(b.label, "ja"));
  return result;
}

function labelName(raw: unknown): string {
  const name = String(raw ?? "").split(/\s+/).filter(Boolean).join(" ");
  if (!name) throw new Error("ラベルの名前を入力してください");
  if ([...name].length > 30) throw new Error("ラベルの名前は30文字までです");
  if (name === GIFT_LABEL) throw new Error("その名前はギフトの印で使っています");
  return name;
}

function findItem(id: number): StoredItem {
  for (const order of state.orders) {
    const item = order.items.find((candidate) => candidate.id === id);
    if (item) return item;
  }
  throw new Error("購入履歴の商品が見つかりません");
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function walk(context: PreviewPurchaseContext): Promise<PurchaseWalkReport> {
  if (!context.builtinBrowser) throw new Error("内蔵ブラウザは設定で有効にしてから使います");
  if (state.running) throw new Error("購入履歴の読み込み中です");
  state.running = true;
  state.cancel = false;
  const job = "browser-purchases";
  const report: PurchaseWalkReport = { pages: 0, added: 0, removed: 0, details: 0, cancelled: false, notice: "" };
  try {
    const fresh = state.orders.length === 0 ? buildOrders() : [];
    const pages = Math.max(1, Math.ceil(fresh.length / 20));
    for (let index = 1; index <= pages; index += 1) {
      context.emitProgress({ job, current: 0, total: 0, message: `購入履歴 ${index}/${pages} ページ（新しい注文 ${report.added} 件）`, state: "running" });
      await wait(250);
      if (state.cancel) break;
      const batch = fresh.slice((index - 1) * 20, index * 20);
      state.orders.push(...batch.map((order) => ({ ...order, detailRead: order.source === "checkout" ? false : order.detailRead })));
      report.added += batch.length;
      report.pages += 1;
      changed();
    }
    const pending = state.orders.filter((order) => !order.detailRead && order.source !== "checkout");
    for (const [index, order] of pending.entries()) {
      if (state.cancel) break;
      context.emitProgress({ job, current: index + 1, total: pending.length, message: `注文の詳細 ${index + 1}/${pending.length}`, state: "running" });
      await wait(30);
      order.detailRead = true;
      report.details += 1;
    }
    report.cancelled = state.cancel;
    if (!report.cancelled) state.complete = true;
    state.lastReadAt = new Date().toISOString();
    const summaryLine = `購入履歴 ${report.pages} ページ、新しい注文 ${report.added} 件、詳細 ${report.details} 件`;
    context.emitProgress({ job, current: 1, total: 1, message: summaryLine, state: report.cancelled ? "cancelled" : "done" });
    return report;
  } finally {
    state.running = false;
    changed();
  }
}

/** 購入履歴のコマンドだけを受ける。ほかのコマンドなら `handled: false`。 */
export async function previewPurchaseDispatch(
  command: string,
  args: Record<string, unknown>,
  context: PreviewPurchaseContext,
): Promise<{ handled: boolean; value?: unknown }> {
  switch (command) {
    case "purchase_history":
      return { handled: true, value: page((args.query ?? {}) as PurchaseQuery) };
    case "purchase_summary":
      return { handled: true, value: summary((args.year as number | null) ?? null) };
    case "purchase_status":
      return { handled: true, value: status() };
    case "import_booth_purchases":
      return { handled: true, value: await walk(context) };
    case "cancel_booth_purchases":
      if (state.running) state.cancel = true;
      return { handled: true };
    case "set_purchase_show_prices":
      state.showPrices = Boolean(args.show);
      changed();
      return { handled: true };
    case "create_purchase_label": {
      const name = labelName(args.name);
      if (state.labels.some((label) => label.name === name)) throw new Error("同じ名前のラベルがあります");
      if (state.labels.length >= 100) throw new Error("ラベルは100個までです");
      const label = { id: state.nextLabel++, name };
      state.labels.push(label);
      return { handled: true, value: { ...label, items: 0 } };
    }
    case "rename_purchase_label": {
      const name = labelName(args.name);
      const label = state.labels.find((candidate) => candidate.id === args.id);
      if (!label) throw new Error("ラベルが見つかりません");
      if (state.labels.some((candidate) => candidate.name === name && candidate.id !== label.id)) throw new Error("同じ名前のラベルがあります");
      label.name = name;
      return { handled: true };
    }
    case "delete_purchase_label": {
      const before = state.labels.length;
      state.labels = state.labels.filter((label) => label.id !== args.id);
      if (state.labels.length === before) throw new Error("ラベルが見つかりません");
      for (const order of state.orders) for (const item of order.items) item.labelIds = item.labelIds.filter((id) => id !== args.id);
      return { handled: true };
    }
    case "set_purchase_item_labels": {
      const item = findItem(Number(args.itemId));
      const ids = [...new Set((args.labelIds as number[]) ?? [])];
      if (ids.some((id) => !state.labels.some((label) => label.id === id))) throw new Error("ラベルが見つかりません");
      item.labelIds = ids;
      return { handled: true };
    }
    case "set_purchase_gift": {
      const order = state.orders.find((candidate) => candidate.orderId === args.orderId);
      if (!order) throw new Error("注文が見つかりません");
      order.giftOverride = args.gift == null ? null : Boolean(args.gift);
      return { handled: true };
    }
    default:
      return { handled: false };
  }
}

/** データ移行の見本に入れる購入履歴。 */
export function previewPurchaseExport(): { purchases: unknown; orders: number; items: number } {
  const names = new Map(state.labels.map((label) => [label.id, label.name]));
  const orders = [...state.orders]
    .sort((a, b) => a.orderId.length - b.orderId.length || a.orderId.localeCompare(b.orderId))
    .map((order) => ({
      orderId: order.orderId,
      orderedAt: order.orderedAt,
      totalYen: order.totalYen,
      gift: gift(order),
      giftDetected: order.giftDetected,
      giftOverride: order.giftOverride,
      detailRead: order.detailRead,
      items: order.items.map((item) => ({
        boothId: item.sample.boothId,
        name: item.sample.name,
        shop: item.sample.shop,
        priceYen: item.sample.priceYen,
        boostYen: item.sample.boostYen,
        quantity: 1,
        labels: item.labelIds.map((id) => names.get(id) ?? "").filter(Boolean).sort(),
      })),
    }));
  return {
    purchases: { showPrices: state.showPrices, labels: labels().map((label) => label.name), orders },
    orders: orders.length,
    items: orders.reduce((sum, order) => sum + order.items.length, 0),
  };
}
