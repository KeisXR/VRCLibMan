import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import type { UnityProject, UnityProjectList } from "../types";
import { errorText } from "../ui";
import { linkableProjects, projectStateNote } from "../unityProjects";

/** プロジェクトの一覧を読む。変更系の呼び出しは新しい一覧を返すので、それを `apply` で受ける。 */
export function useUnityProjects() {
  const [list, setList] = useState<UnityProjectList | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const reload = useCallback(() => {
    const mine = ++generation.current;
    setLoading(true);
    api.unityProjects().then(
      (value) => {
        if (mine !== generation.current) return;
        setList(value);
        setError(null);
        setLoading(false);
      },
      (err: unknown) => {
        if (mine !== generation.current) return;
        setError(errorText(err));
        setLoading(false);
      },
    );
  }, []);
  useEffect(reload, [reload]);
  const apply = useCallback((value: UnityProjectList) => {
    generation.current += 1;
    setList(value);
    setError(null);
    setLoading(false);
  }, []);
  return { list, loading, error, reload, apply };
}

/** 「開く」。版の合う Unity で開き、開いていれば知らせるだけ。 */
export function useOpenUnityProject() {
  const { notify, fail } = useShell();
  const [opening, setOpening] = useState<string | null>(null);
  const open = useCallback(
    async (project: UnityProject) => {
      setOpening(project.id);
      try {
        const result = await api.openUnityProject(project.id);
        notify(
          result.alreadyOpen
            ? `${project.name} はすでに Unity で開いています`
            : `${project.name} を Unity で開いています。起動には少し時間がかかります`,
        );
      } catch (err) {
        fail(errorText(err));
      } finally {
        setOpening(null);
      }
    },
    [notify, fail],
  );
  return { open, opening };
}

/** アバター詳細の「Unity プロジェクト」。結び付けたプロジェクトを開き、結び付けを足し外しする。 */
export function AvatarUnityProjects({ avatarId }: { avatarId: string }) {
  const { fail } = useShell();
  const { list, apply } = useUnityProjects();
  const { open, opening } = useOpenUnityProject();
  const [busy, setBusy] = useState(false);
  if (!list) return null;
  const linked = list.projects.filter((project) => project.avatars.some((avatar) => avatar.id === avatarId));
  const candidates = linkableProjects(list.projects, avatarId);
  if (linked.length === 0 && candidates.length === 0) return null;

  const link = async (projectId: string, linkedNow: boolean) => {
    setBusy(true);
    try {
      apply(await api.linkUnityProjectAvatar(projectId, avatarId, linkedNow));
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="avatar-projects" aria-label="Unity プロジェクト">
      <span className="avatar-projects-label">Unity プロジェクト</span>
      {linked.map((project) => {
        const note = projectStateNote(project);
        return (
          <span className="avatar-project" key={project.id}>
            <a href={`#/projects/${encodeURIComponent(project.id)}`} title={project.path}>{project.name}</a>
            {note && project.running !== "open" && <span className="meta">（{note}）</span>}
            <button
              className="button secondary"
              type="button"
              disabled={project.state !== "ok" || opening === project.id}
              onClick={() => void open(project)}
            >
              {project.running === "open" ? "開いています" : "開く"}
            </button>
            <button
              className="icon-button"
              type="button"
              aria-label={`${project.name} の結び付けを外す`}
              title="結び付けを外す"
              disabled={busy}
              onClick={() => void link(project.id, false)}
            >
              ×
            </button>
          </span>
        );
      })}
      {candidates.length > 0 && (
        <select
          className="avatar-project-add"
          aria-label="プロジェクトを結び付ける"
          value=""
          disabled={busy}
          onChange={(event) => {
            if (event.target.value) void link(event.target.value, true);
          }}
        >
          <option value="">{linked.length === 0 ? "プロジェクトを結び付ける…" : "ほかのプロジェクトも結び付ける…"}</option>
          {candidates.map((project) => (
            <option key={project.id} value={project.id}>
              {project.name}
              {project.avatars.length > 0 ? `（${project.avatars.map((avatar) => avatar.name).join("・")}）` : ""}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}
