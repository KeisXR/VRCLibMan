import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";

const require = createRequire(import.meta.url);
const policy = require("../extension/request-policy.js");

test("Retry-After respects seconds, HTTP dates and invalid values", () => {
  assert.equal(policy.retryAfterMs("120"), 120000);
  assert.equal(policy.retryAfterMs("0"), 0);
  assert.equal(policy.retryAfterMs("1.5"), 1500);
  assert.equal(policy.retryAfterMs("Wed, 21 Oct 2015 07:28:00 GMT", Date.parse("2015-10-21T07:27:00Z")), 60000);
  assert.equal(policy.retryAfterMs("Wed, 21 Oct 2015 07:28:00 GMT", Date.parse("2015-10-21T07:29:00Z")), 0);
  assert.equal(policy.retryAfterMs(null), null);
  assert.equal(policy.retryAfterMs("-1"), null);
  assert.equal(policy.retryAfterMs("invalid"), null);
});

test("allowed intervals are normalized and unsupported values default to safe mode", () => {
  assert.deepEqual(policy.ALLOWED_INTERVALS_MS, [1000, 500, 250]);
  for (const ms of policy.ALLOWED_INTERVALS_MS) assert.equal(policy.normalizeIntervalMs(ms), ms);
  assert.equal(policy.normalizeIntervalMs("250"), 250);
  assert.equal(policy.normalizeIntervalMs(0), 1000);
  assert.equal(policy.normalizeIntervalMs(1), 1000);
  assert.equal(policy.normalizeIntervalMs("999"), 1000);
  assert.equal(policy.normalizeIntervalMs("invalid"), 1000);
});

test("request pacer enforces a minimum gap even with an instant fetch", async () => {
  let now = 5000;
  const waited = [];
  const pace = policy.createPacer(1000, () => now, async (ms) => {
    waited.push(ms);
    now += ms;
  });
  await pace();
  await pace();
  await pace();
  assert.deepEqual(waited, [1000, 1000]);
  now += 5000;
  await pace();
  assert.deepEqual(waited, [1000, 1000]);
});

test("only rate limiting and temporary unavailability trigger backoff", () => {
  assert.equal(policy.isThrottled(429), true);
  assert.equal(policy.isThrottled(503), true);
  assert.equal(policy.isThrottled(403), false);
  assert.equal(policy.isThrottled(404), false);
  assert.equal(policy.canUseJsonFallback(401), false);
  assert.equal(policy.canUseJsonFallback(403), false);
  assert.equal(policy.canUseJsonFallback(404), true);
  assert.equal(policy.canUseJsonFallback(200), true);
  assert.equal(policy.DEFAULT_INTERVAL_MS >= 1000, true);
  assert.equal(policy.MAX_RETRY_MS > policy.DEFAULT_RETRY_MS, true);
});
