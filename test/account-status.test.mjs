import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

test("account check failures stay distinct from signed out", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const {
      checkingAccount,
      accountAfterLogout,
      applyAccountResult,
      accountLabel,
    } = await server.ssrLoadModule("/src/accountStatus.ts");
    const signedIn = { loggedIn: true, displayName: "テスト利用者", pixivId: "424242", avatarUrl: "" };
    const signedOut = { loggedIn: false, displayName: "", pixivId: "", avatarUrl: "" };

    const checking = checkingAccount();
    assert.equal(accountLabel(checking), "確認中");
    const unknown = applyAccountResult(checking, 1, 1, { ok: false });
    assert.equal(unknown.phase, "unverified");
    assert.equal(unknown.account, null);
    assert.equal(accountLabel(unknown), "状態不明");
    assert.notEqual(accountLabel(unknown), "未ログイン");

    const known = applyAccountResult(unknown, 2, 2, { ok: true, account: signedIn });
    assert.equal(accountLabel(known), "テスト利用者");
    const staleFailure = applyAccountResult(known, 3, 2, { ok: false });
    assert.deepEqual(staleFailure, known);
    const failed = applyAccountResult(known, 3, 3, { ok: false });
    assert.equal(failed.phase, "unverified");
    assert.equal(failed.account.displayName, "テスト利用者");
    assert.equal(failed.account.loggedIn, true);
    assert.equal(accountLabel(failed), "テスト利用者（確認できません）");

    const staleSuccess = applyAccountResult(failed, 4, 3, { ok: true, account: signedOut });
    assert.deepEqual(staleSuccess, failed);
    const explicitOut = applyAccountResult(failed, 4, 4, { ok: true, account: signedOut });
    assert.equal(explicitOut.phase, "known");
    assert.equal(accountLabel(explicitOut), "未ログイン");

    const loggedOut = accountAfterLogout();
    assert.equal(accountLabel(loggedOut), "未ログイン");
    const recheckFailed = applyAccountResult(loggedOut, 5, 5, { ok: false });
    assert.equal(recheckFailed.account.loggedIn, false);
    assert.equal(accountLabel(recheckFailed), "未ログイン（確認できません）");
    const idOnly = applyAccountResult(checking, 6, 6, {
      ok: true,
      account: { loggedIn: true, displayName: "  ", pixivId: "424242", avatarUrl: "" },
    });
    assert.equal(accountLabel(idOnly), "424242");

    const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
    assert.match(app, /applyAccountResult/);
    assert.match(app, /accountAfterLogout/);
    assert.doesNotMatch(app, /loggedIn:\s*false/);
  } finally {
    await server.close();
  }
});
