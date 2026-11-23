import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { AgentChange, AgentClient } from "../types";
import { errorText } from "../ui";
import type { NotifyOptions } from "../notice";
import { SettingRow } from "./SettingRow";

const CONFIG_TEXT: Record<AgentClient["config"], string> = {
  none: "未設定",
  current: "設定済み",
  otherPath: "別の場所の VRCLibMan を指しています",
  unreadable: "設定ファイルを読めません（手動で設定してください）",
  manual: "手動で設定します",
  unknown: "確かめられません",
};

const SKILL_TEXT: Record<AgentClient["skill"], string> = {
  unsupported: "SKILL には対応していません",
  none: "SKILL は入っていません",
  current: "SKILL は入っています",
  outdated: "SKILL が古くなっています",
  foreign: "同じ名前の別の SKILL があります",
};

/** エージェントを並べる順：見つかったもの、確かめたもの、名前。 */
export function sortAgents(agents: AgentClient[]): AgentClient[] {
  return [...agents].sort((a, b) =>
    Number(b.installed) - Number(a.installed) || Number(b.verified) - Number(a.verified) || a.label.localeCompare(b.label));
}

/** 「見つかったエージェントすべてに設定」の対象。 */
export function agentsToConfigure(agents: AgentClient[]): AgentClient[] {
  return agents.filter((agent) => agent.installed && agent.automatic && (agent.config === "none" || agent.config === "otherPath"));
}

function optionLabel(agent: AgentClient): string {
  const marks = [!agent.installed && "見つかりません", !agent.verified && "未確認"].filter(Boolean);
  return marks.length ? `${agent.label}（${marks.join("・")}）` : agent.label;
}

/** 操作の結果の文。`done` は「{名前}に設定しました」の「に設定しました」の部分。 */
export function changeMessage(change: AgentChange, done: string, unchanged: string): string {
  if (!change.changed.length) return `${change.label}${unchanged}`;
  const backup = change.backups.length ? `。元の設定は ${change.backups.join("、")} に残しました` : "";
  return `${change.label}${done}${backup}`;
}

/** 設定 → Unity →「AI エージェント連携」の、エージェントごとの設定と SKILL。 */
export function AgentClients({ onNotify, onFail }: { onNotify: (message: string, action?: undefined, options?: NotifyOptions) => void; onFail: (message: string) => void }) {
  const [agents, setAgents] = useState<AgentClient[] | null>(null);
  const [selected, setSelected] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const next = sortAgents(await api.agentClients());
      setAgents(next);
      setSelected((current) => current || next[0]?.id || "");
    } catch (err) {
      onFail(errorText(err));
    }
  }, [onFail]);

  useEffect(() => { void load(); }, [load]);

  async function run(work: () => Promise<string>) {
    setBusy(true);
    try {
      onNotify(await work(), undefined, { persist: true });
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
      await load();
    }
  }

  const agent = agents?.find((item) => item.id === selected);
  const pending = agents ? agentsToConfigure(agents) : [];

  function configureAll() {
    void run(async () => {
      const done: string[] = [];
      const failed: string[] = [];
      for (const target of pending) {
        try {
          await api.changeAgentConfig(target.id, true);
          done.push(target.label);
        } catch (err) {
          failed.push(`${target.label}：${errorText(err)}`);
        }
      }
      if (failed.length && !done.length) throw new Error(failed.join("\n"));
      return [done.length ? `${done.length} 件に設定しました（${done.join("、")}）。各エージェントを起動し直すと使えます` : "", ...failed].filter(Boolean).join("\n");
    });
  }

  return (
    <>
      <SettingRow id="settings-ai-agent-clients" label="AI エージェントへの登録" help="選んだエージェントの設定ファイルに VRCLibMan を足します。ほかの設定はそのまま残し、書き換える前の内容は「.vrclibman-backup」として隣に残します。">
        <button className="button" type="button" disabled={busy || !pending.length} onClick={configureAll}>
          {pending.length ? `見つかった ${pending.length} 件に設定` : "見つかったエージェントは設定済み"}
        </button>
      </SettingRow>
      <SettingRow id="settings-ai-agent-client" label="エージェント" help="入っていないもの・この PC で確かめていないものには印を付けています。">
        <select aria-label="エージェント" value={selected} disabled={!agents} onChange={(e) => setSelected(e.target.value)}>
          {agents?.map((item) => <option key={item.id} value={item.id}>{optionLabel(item)}</option>)}
        </select>
      </SettingRow>
      {agent && <div className="agent-client" aria-live="polite">
        <p className={`agent-state ${agent.config === "current" ? "ok" : agent.config === "otherPath" || agent.config === "unreadable" ? "warn" : ""}`}>
          <span className="agent-dot" aria-hidden="true" />{CONFIG_TEXT[agent.config]}{agent.skill !== "unsupported" && <span className="meta">・{SKILL_TEXT[agent.skill]}</span>}
        </p>
        {agent.automatic && <div className="inline-control">
          <button className="button" type="button" disabled={busy} onClick={() => void run(async () => changeMessage(await api.changeAgentConfig(agent.id, true), "に設定しました。エージェントを起動し直すと使えます", "は、すでに設定済みです"))}>{agent.config === "otherPath" ? "この VRCLibMan に直す" : "設定する"}</button>
          {(agent.config === "current" || agent.config === "otherPath") && <button className="button secondary" type="button" disabled={busy} onClick={() => void run(async () => changeMessage(await api.changeAgentConfig(agent.id, false), "から外しました", "には設定されていません"))}>外す</button>}
          {agent.skill !== "unsupported" && agent.skill !== "foreign" && <button className="button" type="button" disabled={busy} onClick={() => void run(async () => changeMessage(await api.changeAgentSkill(agent.id, true), "に SKILL を入れました", "に SKILL を入れました"))}>{agent.skill === "outdated" ? "SKILL を更新" : agent.skill === "current" ? "SKILL を入れ直す" : "SKILL を入れる"}</button>}
          {(agent.skill === "current" || agent.skill === "outdated") && <button className="button secondary" type="button" disabled={busy} onClick={() => void run(async () => changeMessage(await api.changeAgentSkill(agent.id, false), "の SKILL を外しました", "には SKILL が入っていません"))}>SKILL を外す</button>}
        </div>}
        <details className="settings-details">
          <summary>手動で設定する</summary>
          {agent.note && <p>{agent.note}</p>}
          {agent.configPath && <p className="meta unity-path">設定ファイル：{agent.configPath}</p>}
          <div className="inline-control"><button className="button" type="button" onClick={() => void navigator.clipboard.writeText(agent.manual).then(() => onNotify("設定をコピーしました"), (err) => onFail(errorText(err)))}>コピー</button></div>
          <pre className="meta agent-config" aria-label="手動で設定する内容">{agent.manual}</pre>
        </details>
      </div>}
    </>
  );
}
