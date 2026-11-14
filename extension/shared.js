// 拡張のどの画面からも使う定数と小さな関数。商品や所持の判断はここに置かない（アプリの Rust が持つ）。
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  root.VrcLibManShared = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const LOCAL_PORT = 17321;
  const LOCAL_BASE = `http://127.0.0.1:${LOCAL_PORT}`;
  const WAKE_URL = "vrclibman://import";
  const WAKE_WAIT_MS = 8000;
  const LIBRARY_URL = "https://accounts.booth.pm/library";
  const ORDERS_URL = "https://accounts.booth.pm/orders";
  const LIBRARY_SECTIONS = [
    { url: "https://accounts.booth.pm/library", label: "購入ライブラリ" },
    { url: "https://accounts.booth.pm/library/gifts", label: "ギフト" },
    { url: "https://accounts.booth.pm/library/free_downloads", label: "無料ダウンロード" },
  ];
  // chrome.storage.local の既定値。購入完了ページの登録と BOOTH 上の表示は既定でオン。
  const DEFAULTS = {
    pairingToken: "",
    routeDownloads: false,
    boothIntervalMs: 1000,
    autoPurchase: true,
    showBadges: true,
  };
  const JOB_LABELS = {
    page: "このページを送信中",
    library: "購入ライブラリを読み込み中",
    "library-new": "新しい購入を確認中",
    purchases: "購入履歴を読み込み中",
  };
  const HISTORY_LABELS = {
    page: "このページ",
    library: "ライブラリ全取得",
    "library-new": "新しい購入",
    purchases: "購入履歴",
    checkout: "購入完了ページ",
  };

  function normalizeToken(value) {
    return String(value || "").replace(/[\s-]/g, "").toUpperCase();
  }

  function displayToken(value) {
    const token = normalizeToken(value);
    return token.length === 8 ? `${token.slice(0, 4)}-${token.slice(4)}` : token;
  }

  function localHeaders(token, version, json) {
    const headers = {
      Authorization: `Bearer ${token}`,
      "X-VRCLibMan-Extension-Version": version,
    };
    if (json) headers["Content-Type"] = "application/json";
    return headers;
  }

  function boothUrl(value) {
    try {
      const url = new URL(value);
      return url.protocol === "https:" && !url.username && !url.password && !url.port && /(^|\.)booth\.pm$/.test(url.hostname)
        ? url
        : null;
    } catch {
      return null;
    }
  }

  // 開いているページの種類。ポップアップが出す操作を決めるだけに使う。
  function pageKind(href) {
    const url = boothUrl(href);
    if (!url) return "other";
    const path = url.pathname.replace(/\/+$/, "");
    if (url.hostname === "accounts.booth.pm") {
      if (path === "/library" || path.startsWith("/library/")) return "library";
      if (path === "/orders" || /^\/orders\/\d{4,12}$/.test(path)) return "orders";
      return "booth";
    }
    if (url.hostname === "checkout.booth.pm") return "booth";
    if (/\/items\/\d{4,12}$/.test(path)) return "item";
    return "booth";
  }

  function librarySectionLabel(href) {
    const url = boothUrl(href);
    const path = url ? url.pathname.replace(/\/+$/, "") : "";
    const section = LIBRARY_SECTIONS.find((row) => new URL(row.url).pathname === path);
    return section ? section.label : "購入ライブラリ";
  }

  function pad(value) {
    return String(value).padStart(2, "0");
  }

  // 「今日 18:02」「昨日 21:15」「11/12 9:00」の形。
  function formatTime(value, now = new Date()) {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "";
    const time = `${date.getHours()}:${pad(date.getMinutes())}`;
    const day = (d) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
    if (day(date) === day(now)) return `今日 ${time}`;
    const yesterday = new Date(now);
    yesterday.setDate(now.getDate() - 1);
    if (day(date) === day(yesterday)) return `昨日 ${time}`;
    return `${date.getMonth() + 1}/${date.getDate()} ${time}`;
  }

  function formatBytes(value) {
    const bytes = Number(value);
    if (value === null || value === undefined || value === "" || !Number.isFinite(bytes) || bytes < 0) return "";
    if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  // 受信口の応答から、接続の印と説明を決める。過去の通信から今の接続は断定しない。
  function connectionState(status, hasToken) {
    if (!hasToken) {
      return { tone: "warn", label: "未接続", message: "VRCLibMan の「設定 → BOOTH」に出るペアリングコードを貼ります。", paired: false };
    }
    if (status === undefined) return { tone: "", label: "確認中", message: "", paired: true };
    if (status >= 200 && status < 300) return { tone: "good", label: "接続中", message: "", paired: true, reachable: true };
    if (status === 401) {
      return { tone: "warn", label: "未接続", message: "コードが一致しません。アプリの設定からコピーし直してください。", paired: false };
    }
    if (status === 403) {
      return { tone: "warn", label: "未接続", message: "この拡張機能は接続を許可されていません。アプリでコードを作り直してください。", paired: false };
    }
    if (status === 429) {
      return { tone: "warn", label: "待機中", message: "接続の試行が続いています。少し待ってから確認してください。", paired: true };
    }
    return { tone: "warn", label: "アプリ未起動", message: "送るときにアプリの起動を試みます。", paired: true };
  }

  return {
    LOCAL_PORT,
    LOCAL_BASE,
    WAKE_URL,
    WAKE_WAIT_MS,
    LIBRARY_URL,
    ORDERS_URL,
    LIBRARY_SECTIONS,
    DEFAULTS,
    JOB_LABELS,
    HISTORY_LABELS,
    normalizeToken,
    displayToken,
    localHeaders,
    boothUrl,
    pageKind,
    librarySectionLabel,
    formatTime,
    formatBytes,
    connectionState,
  };
});
