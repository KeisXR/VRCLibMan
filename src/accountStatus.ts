import type { BoothAccount } from "./types";

export type AccountPhase = "checking" | "known" | "unverified";

/** 最後に確定したアカウントと、直近の確認が失敗しているか。 */
export interface AccountSnapshot {
  account: BoothAccount | null;
  phase: AccountPhase;
}

export type AccountResult = { ok: true; account: BoothAccount } | { ok: false };

export function checkingAccount(): AccountSnapshot {
  return { account: null, phase: "checking" };
}

export function accountAfterLogout(): AccountSnapshot {
  return {
    account: { loggedIn: false, displayName: "", pixivId: "", avatarUrl: "" },
    phase: "known",
  };
}

/**
 * 新しい確認だけを反映する。失敗は直前の確定表示を残し、未ログインには置き換えない。
 * 一度も確定していなければアカウントは空のままにする。
 */
export function applyAccountResult(
  previous: AccountSnapshot,
  currentGeneration: number,
  ticket: number,
  result: AccountResult,
): AccountSnapshot {
  if (ticket !== currentGeneration) return previous;
  if (result.ok) return { account: result.account, phase: "known" };
  return { account: previous.account, phase: "unverified" };
}

export function accountLabel(snapshot: AccountSnapshot): string {
  if (snapshot.phase === "unverified" && !snapshot.account) return "状態不明";
  if (snapshot.phase === "unverified") {
    return `${definitiveLabel(snapshot.account)}（確認できません）`;
  }
  if (!snapshot.account || snapshot.phase === "checking") return "確認中";
  return definitiveLabel(snapshot.account);
}

function definitiveLabel(account: BoothAccount | null): string {
  if (!account || !account.loggedIn) return "未ログイン";
  const name = account.displayName.trim();
  if (name) return name;
  const pixivId = account.pixivId.trim();
  if (pixivId) return pixivId;
  return "ログイン中";
}
