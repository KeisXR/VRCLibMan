(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  root.VrcLibManRequestPolicy = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const DEFAULT_INTERVAL_MS = 1000;
  const ALLOWED_INTERVALS_MS = [1000, 500, 250];

  function normalizeIntervalMs(value) {
    const number = Number(value);
    return ALLOWED_INTERVALS_MS.includes(number) ? number : DEFAULT_INTERVAL_MS;
  }
  const DEFAULT_RETRY_MS = 10000;
  const MAX_RETRY_MS = 60000;

  // Retry-After can be a non-negative number of seconds or an HTTP-date (GMT).
  // Never silently cap a longer server-requested delay to a shorter one.
  // Anything else (negative, signed, exponent, ISO dates, garbage) is null so the
  // caller falls back to its default wait instead of retrying immediately.
  const HTTP_DATE_PATTERNS = [
    // IMF-fixdate: Sun, 06 Nov 1994 08:49:37 GMT
    /^(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun), \d{2} (?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{4} \d{2}:\d{2}:\d{2} GMT$/,
    // obsolete RFC 850: Sunday, 06-Nov-94 08:49:37 GMT
    /^(?:Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday), \d{2}-(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)-\d{2} \d{2}:\d{2}:\d{2} GMT$/,
  ];

  function retryAfterMs(header, nowMs = Date.now()) {
    if (typeof header !== "string" || !header.trim()) return null;
    const value = header.trim();
    if (/^\d+(?:\.\d+)?$/.test(value)) {
      const milliseconds = Number(value) * 1000;
      return Number.isFinite(milliseconds) ? Math.ceil(milliseconds) : null;
    }
    if (!HTTP_DATE_PATTERNS.some((pattern) => pattern.test(value))) return null;
    const deadline = Date.parse(value);
    return Number.isFinite(deadline) ? Math.max(0, deadline - nowMs) : null;
  }

  function createPacer(
    intervalMs = DEFAULT_INTERVAL_MS,
    now = () => Date.now(),
    pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  ) {
    let nextAt = 0;
    return async function waitForTurn() {
      const current = now();
      const delay = Math.max(0, nextAt - current);
      nextAt = Math.max(current, nextAt) + intervalMs;
      if (delay > 0) await pause(delay);
    };
  }

  function isThrottled(status) {
    return status === 429 || status === 503;
  }

  function canUseJsonFallback(htmlStatus) {
    return htmlStatus !== 401 && htmlStatus !== 403;
  }

  return {
    DEFAULT_INTERVAL_MS, ALLOWED_INTERVALS_MS, normalizeIntervalMs,
    DEFAULT_RETRY_MS, MAX_RETRY_MS,
    retryAfterMs, createPacer, isThrottled, canUseJsonFallback,
  };
});
