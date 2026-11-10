import { useCallback, useDeferredValue, useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { api, listenProgress, listenPurchasesChanged, openBoothLink } from "../api";
import { useShell } from "../App";
import {
  GIFT_LABEL,
  PURCHASE_PAGE_SIZE,
  formatPurchaseTime,
  formatReadAt,
  formatYen,
  giftChoice,
  giftValue,
  purchasesHref,
  walkSummary,
} from "../purchases";
import type { GiftChoice, PurchaseView } from "../purchases";
import type {
  PurchaseItemView,
  PurchaseLabel,
  PurchaseOrderView,
  PurchasePage,
  PurchaseQuery,
  PurchaseStatus,
  PurchaseSummary,
} from "../types";
import { Pager, Thumb, errorText } from "../ui";

const JOB = "browser-purchases";

function useChanges(): [number, () => void] {
  const [changes, setChanges] = useState(0);
  const bump = useCallback(() => setChanges((value) => value + 1), []);
  useEffect(() => {
    let alive = true;
    let stop = () => {};
    void listenPurchasesChanged(bump).then((unlisten) => {
      if (!alive) unlisten();
      else stop = unlisten;
    });
    return () => {
      alive = false;
      stop();
    };
  }, [bump]);
  return [changes, bump];
}

function PurchaseGuide({ builtinBrowser }: { builtinBrowser: boolean }) {
  return (
    <div className="empty purchase-guide">
      <p>
        <strong>購入履歴はまだ読み込んでいません。</strong>読み込まなくても、ほかの機能はそのまま使えます。
      </p>
      {builtinBrowser ? (
        <>
          <p>
            「購入履歴を読み込む」を押すと、内蔵ブラウザで BOOTH の購入履歴を 1 ページずつ開き、そのあと注文の詳細を 1 件ずつ読みます。
            ギフトとして贈ったかどうかは詳細にしか出ないことがあるので、すべての注文の詳細を読みます。読み込みのあいだ、内蔵ブラウザのタブが購入履歴のページを移動します。
          </p>
          <p>
            アクセスの間隔は「ライブラリ全取得」と同じ設定（初期値 1 秒）です。注文が多いと時間がかかります。途中で止めても、次は続きから読みます。購入やファイルのダウンロードはしません。
          </p>
          <p>保存するのは、注文番号、注文日時、金額、商品名、ショップ名、ギフトかどうかです。支払方法、住所、ギフト用 URL は保存しません。</p>
          <p>このあとの購入は、内蔵ブラウザで購入完了ページを開いたときに記録します（設定の「取得したアセットの自動登録」がオンのとき）。</p>
        </>
      ) : (
        <>
          <p>購入履歴の読み込みには内蔵ブラウザを使います。設定の「BOOTH の読み込み方」で内蔵ブラウザを選んでから、ここに戻ってください。</p>
          <div className="row-actions">
            <a className="button" href="#/settings">
              設定を開く
            </a>
          </div>
        </>
      )}
    </div>
  );
}

function LabelPicker({ item, labels, onChanged }: { item: PurchaseItemView; labels: PurchaseLabel[]; onChanged: () => void }) {
  const { fail } = useShell();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDetailsElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  async function save(labelIds: number[]) {
    setBusy(true);
    try {
      await api.setPurchaseItemLabels(item.id, labelIds);
      onChanged();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function addNew(event: FormEvent) {
    event.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    try {
      const label = await api.createPurchaseLabel(name);
      await api.setPurchaseItemLabels(item.id, [...item.labelIds, label.id]);
      setName("");
      onChanged();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <details className="purchase-label-picker" ref={ref} open={open} onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>ラベル{item.labelIds.length > 0 ? `（${item.labelIds.length}）` : ""}</summary>
      <div className="purchase-label-panel">
        {labels.length === 0 && <p className="meta">まだラベルはありません。下で作れます。</p>}
        {labels.map((label) => (
          <label key={label.id}>
            <input
              type="checkbox"
              disabled={busy}
              checked={item.labelIds.includes(label.id)}
              onChange={(event) =>
                void save(
                  event.target.checked ? [...item.labelIds, label.id] : item.labelIds.filter((id) => id !== label.id),
                )
              }
            />
            {label.name}
          </label>
        ))}
        <form className="purchase-label-new" onSubmit={(event) => void addNew(event)}>
          <input value={name} maxLength={30} placeholder="新しいラベル" aria-label="新しいラベル" onChange={(event) => setName(event.target.value)} />
          <button className="button" type="submit" disabled={busy || !name.trim()}>
            追加
          </button>
        </form>
      </div>
    </details>
  );
}

function PurchaseItemRow({
  item,
  labels,
  showPrices,
  onChanged,
}: {
  item: PurchaseItemView;
  labels: PurchaseLabel[];
  showPrices: boolean;
  onChanged: () => void;
}) {
  const { fail } = useShell();
  const names = new Map(labels.map((label) => [label.id, label.name]));
  const open = () => void openBoothLink(`https://booth.pm/ja/items/${item.boothId}`).catch((err) => fail(errorText(err)));
  return (
    <li className="purchase-item">
      <Thumb path={item.thumbnailPath} url={item.thumbnailUrl} name={item.name} small />
      <div className="purchase-item-main">
        <button className="purchase-item-name" type="button" title="BOOTH の商品ページを開く" onClick={open}>
          {item.name}
        </button>
        {item.quantity > 1 && <span className="meta"> × {item.quantity}</span>}
        <div className="meta">
          {item.shop}
          {!item.inLibrary && <span>・ライブラリに無い商品</span>}
        </div>
        <div className="pills">
          {item.groups.map((group) =>
            group.avatarId ? (
              <a key={group.key} className="chip" href={`#/avatar/${encodeURIComponent(group.avatarId)}`}>
                {group.label}
                {group.body ? "（本体）" : ""}
              </a>
            ) : (
              <span key={group.key} className="chip">
                {group.label}
              </span>
            ),
          )}
          {item.labelIds
            .filter((id) => names.has(id))
            .map((id) => (
              <span key={`label-${id}`} className="chip purchase-label-chip">
                {names.get(id)}
              </span>
            ))}
        </div>
      </div>
      <div className="purchase-item-side">
        {showPrices && (
          <div className="purchase-price">
            {formatYen(item.priceYen)}
            {item.boostYen ? <span className="meta">BOOST {formatYen(item.boostYen)}</span> : null}
          </div>
        )}
        <LabelPicker item={item} labels={labels} onChanged={onChanged} />
      </div>
    </li>
  );
}

function OrderCard({
  order,
  labels,
  showPrices,
  onChanged,
}: {
  order: PurchaseOrderView;
  labels: PurchaseLabel[];
  showPrices: boolean;
  onChanged: () => void;
}) {
  const { fail } = useShell();
  const time = formatPurchaseTime(order.orderedAt);

  async function changeGift(choice: GiftChoice) {
    try {
      await api.setPurchaseGift(order.orderId, giftValue(choice));
      onChanged();
    } catch (err) {
      fail(errorText(err));
    }
  }

  return (
    <article className="purchase-order">
      <header className="purchase-order-head">
        <div className="purchase-order-when">
          {time ? (
            <time dateTime={order.orderedAt ?? undefined}>{time}</time>
          ) : (
            <span>
              {formatPurchaseTime(order.recordedAt)}
              <span className="meta">（購入完了を記録した日時）</span>
            </span>
          )}
          <span className="meta">注文番号 {order.orderId}</span>
          {order.gift && <span className="badge purchase-gift-badge">{GIFT_LABEL}</span>}
          {!order.detailRead && <span className="badge">詳細は未取得</span>}
        </div>
        <div className="purchase-order-side">
          {showPrices && order.totalYen != null && <span className="purchase-total">お支払 {formatYen(order.totalYen)}</span>}
          <select
            className="purchase-gift-select"
            aria-label={`注文 ${order.orderId} のギフトの扱い`}
            value={giftChoice(order)}
            onChange={(event) => void changeGift(event.target.value as GiftChoice)}
          >
            <option value="auto">{order.giftDetected ? "ギフト（自動判定）" : "自分用（自動判定）"}</option>
            <option value="gift">贈ったギフト</option>
            <option value="self">自分用</option>
          </select>
        </div>
      </header>
      {order.items.length > 0 ? (
        <ul className="purchase-items">
          {order.items.map((item) => (
            <PurchaseItemRow key={item.id} item={item} labels={labels} showPrices={showPrices} onChanged={onChanged} />
          ))}
        </ul>
      ) : (
        <p className="meta purchase-order-pending">{order.title || "商品"}（商品の詳細は未取得です）</p>
      )}
    </article>
  );
}

function LabelManager({ labels, onChanged }: { labels: PurchaseLabel[]; onChanged: () => void }) {
  const { fail, notify } = useShell();
  const [name, setName] = useState("");

  async function create(event: FormEvent) {
    event.preventDefault();
    try {
      await api.createPurchaseLabel(name);
      setName("");
      onChanged();
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function rename(label: PurchaseLabel, next: string) {
    if (next.trim() === label.name) return;
    try {
      await api.renamePurchaseLabel(label.id, next);
      onChanged();
    } catch (err) {
      fail(errorText(err));
      onChanged();
    }
  }

  async function remove(label: PurchaseLabel) {
    if (label.items > 0 && !window.confirm(`ラベル「${label.name}」を消しますか？ ${label.items} 件の商品から外れます。`)) return;
    try {
      await api.deletePurchaseLabel(label.id);
      notify(`ラベル「${label.name}」を消しました`);
      onChanged();
    } catch (err) {
      fail(errorText(err));
    }
  }

  return (
    <details className="settings-details purchase-labels">
      <summary>ラベルを管理（{labels.length}）</summary>
      <p className="meta">ラベルは商品ごとに付けます。「{GIFT_LABEL}」は注文ごとの印で、各注文の右上で切り替えます。</p>
      {labels.length > 0 && (
        <ul className="purchase-label-list">
          {labels.map((label) => (
            <li key={`${label.id}-${label.name}`}>
              <input
                defaultValue={label.name}
                maxLength={30}
                aria-label={`ラベル「${label.name}」の名前`}
                onBlur={(event) => void rename(label, event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") event.currentTarget.blur();
                }}
              />
              <span className="meta">{label.items} 件</span>
              <button className="button" type="button" onClick={() => void remove(label)}>
                削除
              </button>
            </li>
          ))}
        </ul>
      )}
      <form className="purchase-label-new" onSubmit={(event) => void create(event)}>
        <input value={name} maxLength={30} placeholder="新しいラベル" aria-label="新しいラベルの名前" onChange={(event) => setName(event.target.value)} />
        <button className="button" type="submit" disabled={!name.trim()}>
          追加
        </button>
      </form>
    </details>
  );
}

function PurchaseHistoryView({
  initialGroup,
  changes,
  onChanged,
}: {
  initialGroup?: string;
  changes: number;
  onChanged: () => void;
}) {
  const { fail } = useShell();
  const [text, setText] = useState("");
  const query = useDeferredValue(text.trim());
  const [labelFilter, setLabelFilter] = useState("");
  const [group, setGroup] = useState(initialGroup ?? "");
  const [year, setYear] = useState("");
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<PurchasePage | null>(null);
  const top = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setGroup(initialGroup ?? "");
    setOffset(0);
  }, [initialGroup]);

  useEffect(() => {
    setOffset(0);
  }, [query]);

  useEffect(() => {
    let alive = true;
    const request: PurchaseQuery = {
      query,
      offset,
      limit: PURCHASE_PAGE_SIZE,
      labelId: labelFilter && labelFilter !== "gift" ? Number(labelFilter) : null,
      gift: labelFilter === "gift" ? true : null,
      group: group || null,
      year: year ? Number(year) : null,
    };
    void api.purchaseHistory(request).then(
      (next) => {
        if (alive) setPage(next);
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [query, offset, labelFilter, group, year, changes, fail]);

  function turn(next: number) {
    setOffset(next);
    top.current?.scrollIntoView({ block: "start" });
  }

  // アバター別から開いた絞り込みはアドレスに入っている。変えるときはアドレスも合わせ、戻る操作と食い違わないようにする。
  function chooseGroup(next: string) {
    if (initialGroup !== undefined) {
      window.location.hash = purchasesHref("history", next || undefined);
      return;
    }
    setGroup(next);
    setOffset(0);
  }

  if (!page) return <p className="meta">読み込み中</p>;
  const filtered = Boolean(query || labelFilter || group || year);
  return (
    <div className="purchase-history" ref={top}>
      <div className="purchase-filters">
        <input
          className="search"
          type="search"
          value={text}
          placeholder="商品名・ショップ・注文番号・ラベル"
          aria-label="購入履歴を検索"
          onChange={(event) => setText(event.target.value)}
        />
        <label className="field">
          <span>ラベル</span>
          <select value={labelFilter} onChange={(event) => { setLabelFilter(event.target.value); setOffset(0); }}>
            <option value="">すべて</option>
            <option value="gift">{GIFT_LABEL}</option>
            {page.labels.map((label) => (
              <option key={label.id} value={String(label.id)}>
                {label.name}（{label.items}）
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>アバター</span>
          <select value={group} onChange={(event) => chooseGroup(event.target.value)}>
            <option value="">すべて</option>
            {group && !page.groups.some((option) => option.key === group) && <option value={group}>（選択中）</option>}
            {page.groups.map((option) => (
              <option key={option.key} value={option.key}>
                {option.label}（{option.items}）
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>年</span>
          <select value={year} onChange={(event) => { setYear(event.target.value); setOffset(0); }}>
            <option value="">すべて</option>
            {page.years.map((value) => (
              <option key={value} value={String(value)}>
                {value}年
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="meta purchase-count">
        {filtered ? `条件に合う注文 ${page.total} 件` : `注文 ${page.total} 件`}
        {filtered && (
          <button
            className="purchase-link"
            type="button"
            onClick={() => {
              setText("");
              setLabelFilter("");
              setYear("");
              chooseGroup("");
            }}
          >
            条件を外す
          </button>
        )}
      </p>
      {page.orders.length === 0 ? (
        <p className="empty">条件に合う注文はありません。</p>
      ) : (
        <div className="stack">
          {page.orders.map((order) => (
            <OrderCard key={order.orderId} order={order} labels={page.labels} showPrices={page.status.showPrices} onChanged={onChanged} />
          ))}
        </div>
      )}
      <Pager offset={page.offset} pageSize={page.limit} total={page.total} onChange={turn} />
      <LabelManager labels={page.labels} onChanged={onChanged} />
    </div>
  );
}

function PurchaseSummaryView({ changes }: { changes: number }) {
  const { fail } = useShell();
  const [year, setYear] = useState<number | null>(null);
  const [summary, setSummary] = useState<PurchaseSummary | null>(null);

  useEffect(() => {
    let alive = true;
    void api.purchaseSummary(year).then(
      (next) => {
        if (alive) setSummary(next);
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [year, changes, fail]);

  if (!summary) return <p className="meta">読み込み中</p>;
  const prices = summary.showPrices;
  const largest = Math.max(1, ...summary.groups.map((group) => (prices ? group.amountYen : group.items)));
  return (
    <div className="purchase-summary">
      <div className="purchase-filters">
        <label className="field">
          <span>期間</span>
          <select value={year ?? ""} onChange={(event) => setYear(event.target.value ? Number(event.target.value) : null)}>
            <option value="">全期間</option>
            {summary.years.map((value) => (
              <option key={value} value={String(value)}>
                {value}年
              </option>
            ))}
          </select>
        </label>
      </div>
      <dl className="purchase-totals">
        <div>
          <dt>注文</dt>
          <dd>{summary.orders} 件</dd>
        </div>
        <div>
          <dt>商品</dt>
          <dd>{summary.items} 件</dd>
        </div>
        {prices && (
          <div>
            <dt>商品代金と BOOST の合計</dt>
            <dd>{formatYen(summary.totalYen)}</dd>
          </div>
        )}
        {summary.giftItems > 0 && (
          <div>
            <dt>うち贈ったギフト</dt>
            <dd>
              {summary.giftItems} 件{prices ? `・${formatYen(summary.giftYen)}` : ""}
            </dd>
          </div>
        )}
      </dl>
      {summary.groups.length === 0 ? (
        <p className="empty">この期間の購入はありません。</p>
      ) : (
        <table className="purchase-summary-table">
          <thead>
            <tr>
              <th scope="col">アバター</th>
              <th scope="col">商品</th>
              {prices && <th scope="col">金額</th>}
            </tr>
          </thead>
          <tbody>
            {summary.groups.map((group) => {
              const value = prices ? group.amountYen : group.items;
              return (
                <tr key={group.key}>
                  <th scope="row">
                    <a href={purchasesHref("history", group.key)} title="この枠の購入を履歴で見る">
                      {group.label}
                    </a>
                    {group.kind === "avatar" && !group.owned && <span className="meta">（未所持）</span>}
                    {group.bodyItems > 0 && <span className="meta">（本体を含む）</span>}
                    <span className="purchase-bar" aria-hidden="true" style={{ width: `${Math.round((value / largest) * 100)}%` }} />
                  </th>
                  <td>{group.items} 件</td>
                  {prices && (
                    <td>
                      {formatYen(group.amountYen)}
                      {group.unknownAmountItems > 0 && <span className="meta">（金額不明 {group.unknownAmountItems} 件）</span>}
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      <p className="meta">
        対応アバターと汎用は、今の分類から数えます。分類を直すと集計も変わります。複数のアバターに対応する商品は、それぞれのアバターに全額で数え、上の合計では 1 回だけ数えます。
        同じ素体のほかのアバターには広げません。贈ったギフトはアバター別に入れません。送料と手数料は含みません。
        {summary.unknownAmountItems > 0 && prices && ` 金額がまだ分からない商品が ${summary.unknownAmountItems} 件あります。`}
      </p>
    </div>
  );
}

export function PurchasesPage({ view, group }: { view: PurchaseView; group?: string }) {
  const { notify, fail, builtinBrowser, refresh } = useShell();
  const [changes, bump] = useChanges();
  const [status, setStatus] = useState<PurchaseStatus | null>(null);
  const [starting, setStarting] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    let alive = true;
    void api.purchaseStatus().then(
      (next) => {
        if (alive) setStatus(next);
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [changes, fail]);

  useEffect(() => {
    let alive = true;
    let stop = () => {};
    void listenProgress((progress) => {
      if (!alive || progress.job !== JOB) return;
      setMessage(progress.message);
      if (progress.state !== "running") bump();
    }).then((unlisten) => {
      if (!alive) unlisten();
      else stop = unlisten;
    });
    return () => {
      alive = false;
      stop();
    };
  }, [bump]);

  const running = starting || Boolean(status?.running);

  async function read() {
    setStarting(true);
    setMessage("");
    try {
      const report = await api.importBoothPurchases();
      const counts = walkSummary(report);
      if (report.notice) fail(`${counts}。${report.notice}`);
      else if (report.cancelled) notify(`中止しました。${counts}`);
      else notify(`購入履歴を読み込みました。${counts}`);
    } catch (err) {
      fail(errorText(err));
    } finally {
      setStarting(false);
      bump();
      refresh();
    }
  }

  async function togglePrices(show: boolean) {
    try {
      await api.setPurchaseShowPrices(show);
      bump();
    } catch (err) {
      fail(errorText(err));
    }
  }

  const empty = status != null && status.orders === 0;
  return (
    <section className="purchases-page">
      <div className="page-head">
        <div>
          <h1>購入履歴</h1>
          <p className="meta">
            BOOTH で購入した注文を、新しい順に並べます。
            {status?.lastReadAt ? ` 最後に読み込んだ日時: ${formatReadAt(status.lastReadAt)}` : ""}
          </p>
        </div>
        <div className="row-actions">
          {status && !empty && (
            <label className="switch-label purchase-price-toggle">
              <input type="checkbox" checked={status.showPrices} onChange={(event) => void togglePrices(event.target.checked)} />
              <span>金額を表示</span>
            </label>
          )}
          {builtinBrowser && (
            <button className="button primary" type="button" disabled={running} onClick={() => void read()}>
              購入履歴を読み込む
            </button>
          )}
          {running && (
            <button className="button" type="button" onClick={() => void api.cancelBoothPurchases().catch((err) => fail(errorText(err)))}>
              中止
            </button>
          )}
        </div>
      </div>
      {running && <p className="meta purchase-progress">{message || "購入履歴を読み込んでいます"}</p>}
      {!status ? (
        <p className="meta">読み込み中</p>
      ) : empty && !running ? (
        <PurchaseGuide builtinBrowser={builtinBrowser} />
      ) : (
        <>
          <nav className="tabs" aria-label="購入履歴の表示">
            <a className={view === "history" ? "tab active" : "tab"} href={purchasesHref("history")} aria-current={view === "history" ? "page" : undefined}>
              履歴
            </a>
            <a className={view === "summary" ? "tab active" : "tab"} href={purchasesHref("summary")} aria-current={view === "summary" ? "page" : undefined}>
              アバター別
            </a>
          </nav>
          {status.pendingDetails > 0 && !running && (
            <p className="meta purchase-pending">
              詳細をまだ読んでいない注文が {status.pendingDetails} 件あります。
              {builtinBrowser ? "「購入履歴を読み込む」で読み込みます。" : "内蔵ブラウザを有効にすると読み込めます。"}
            </p>
          )}
          {view === "summary" ? (
            <PurchaseSummaryView changes={changes} />
          ) : (
            <PurchaseHistoryView initialGroup={group} changes={changes} onChanged={bump} />
          )}
        </>
      )}
    </section>
  );
}
