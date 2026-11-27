import { useEffect, useMemo, useState } from "react";
import { api, chooseFolder, revealInFolder } from "../api";
import { useShell } from "../App";
import { useOpenUnityProject, useUnityProjects } from "../components/UnityProjects";
import type { AvatarCard, UnityProject } from "../types";
import { errorText } from "../ui";
import { PROJECT_KIND_LABEL, canOpenProject, filterProjects, modifiedLabel, projectStateNote } from "../unityProjects";

/** Unity プロジェクトの一覧（docs/unity-projects.md の段階 1）。フォルダは読むだけ。 */
export function ProjectsPage() {
  const { fail } = useShell();
  const { list, loading, error, reload, apply } = useUnityProjects();
  const { open, opening } = useOpenUnityProject();
  const [query, setQuery] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [avatars, setAvatars] = useState<AvatarCard[]>([]);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    api.avatars().then(
      (cards) => setAvatars(cards.filter((card) => card.owned)),
      () => setAvatars([]),
    );
  }, []);

  const projects = useMemo(() => filterProjects(list?.projects ?? [], query, showHidden), [list, query, showHidden]);
  const hiddenCount = list?.projects.filter((project) => project.hidden).length ?? 0;

  const run = async (key: string, work: () => Promise<void>) => {
    setBusy(key);
    try {
      await work();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(null);
    }
  };

  const addProject = () =>
    run("add", async () => {
      const path = await chooseFolder();
      if (path) apply(await api.addUnityProject(path));
    });

  return (
    <section className="projects-page">
      <div className="page-head">
        <div>
          <h1>プロジェクト</h1>
          <p>ALCOM や VCC のプロジェクトを、アバターと結び付けて開けます。プロジェクトの中身は読むだけで、書き換えません。</p>
        </div>
        <div className="button-row">
          <button className="button secondary" type="button" onClick={reload} disabled={loading}>
            読み直す
          </button>
          <button className="button secondary" type="button" onClick={() => void addProject()} disabled={busy === "add"}>
            フォルダを選んで追加
          </button>
        </div>
      </div>

      {error && <p className="note note-strong">{error}</p>}
      {list && !list.alcomFound && (
        <div className="empty">
          <p>ALCOM か VCC のプロジェクトが見つかりません。ALCOM を使っていないときは、「フォルダを選んで追加」でプロジェクトを足せます。</p>
          <p className="meta">読んだ場所: {list.alcomPath || "（不明）"}</p>
        </div>
      )}

      {list && list.projects.length > 0 && (
        <div className="toolbar projects-toolbar">
          <input
            className="search"
            value={query}
            placeholder="プロジェクト名・アバターで絞り込む"
            aria-label="プロジェクトを絞り込む"
            onChange={(event) => setQuery(event.target.value)}
          />
          {hiddenCount > 0 && (
            <label className="switch-label">
              <input type="checkbox" checked={showHidden} onChange={(event) => setShowHidden(event.target.checked)} />
              隠したものも表示（{hiddenCount}）
            </label>
          )}
        </div>
      )}

      {loading && !list && <p className="meta">プロジェクトを読んでいます…</p>}
      {list && list.alcomFound && list.projects.length === 0 && (
        <div className="empty">
          <p>ALCOM にプロジェクトがありません。ALCOM でプロジェクトを作るか、「フォルダを選んで追加」で足してください。</p>
        </div>
      )}
      {list && list.projects.length > 0 && projects.length === 0 && <p className="meta">当てはまるプロジェクトはありません。</p>}

      <ul className="project-list">
        {projects.map((project) => (
          <ProjectRow
            key={project.id}
            project={project}
            avatars={avatars}
            busy={busy === project.id}
            opening={opening === project.id}
            onOpen={() => void open(project)}
            onReveal={() => void revealInFolder(project.path).catch((err: unknown) => fail(errorText(err)))}
            onHide={(hidden) => void run(project.id, async () => apply(await api.setUnityProjectHidden(project.id, hidden)))}
            onRemove={() => void run(project.id, async () => apply(await api.removeUnityProject(project.id)))}
            onLink={(avatarId, linked) =>
              void run(project.id, async () => apply(await api.linkUnityProjectAvatar(project.id, avatarId, linked)))
            }
          />
        ))}
      </ul>
      {list?.alcomFound && <p className="meta projects-source">ALCOM / VCC の一覧（{list.alcomPath}）を読んでいます。</p>}
    </section>
  );
}

function ProjectRow({
  project,
  avatars,
  busy,
  opening,
  onOpen,
  onReveal,
  onHide,
  onRemove,
  onLink,
}: {
  project: UnityProject;
  avatars: AvatarCard[];
  busy: boolean;
  opening: boolean;
  onOpen: () => void;
  onReveal: () => void;
  onHide: (hidden: boolean) => void;
  onRemove: () => void;
  onLink: (avatarId: string, linked: boolean) => void;
}) {
  const note = projectStateNote(project);
  const linkedIds = new Set(project.avatars.map((avatar) => avatar.id));
  // ワールドのプロジェクトはアバターに結び付けない。ワールドの商品の「Unityで開く」で、結び付けなしに候補になる。
  const linkable = project.kind === "world" ? [] : avatars.filter((avatar) => !linkedIds.has(avatar.id));
  const modified = modifiedLabel(project.modified);
  return (
    <li className={project.hidden ? "project-row hidden-project" : "project-row"}>
      <div className="project-main">
        <div className="project-title">
          <strong title={project.path}>{project.name}</strong>
          {project.state === "ok" && <span className="badge">{PROJECT_KIND_LABEL[project.kind]}</span>}
          {project.running === "open" && <span className="badge project-open">開いています</span>}
          {project.source !== "alcom" && <span className="badge">{project.source === "manual" ? "手で追加" : "ALCOM の一覧に無し"}</span>}
          {project.hidden && <span className="badge">隠しています</span>}
        </div>
        <p className="meta project-meta">
          {[project.version && `Unity ${project.version}`, modified && `更新 ${modified}`, note && note !== "開いています" ? note : null]
            .filter(Boolean)
            .join("・")}
        </p>
        <div className="project-avatars">
          {project.avatars.map((avatar) => (
            <span className="chip project-avatar" key={avatar.id}>
              <a href={`#/avatar/${encodeURIComponent(avatar.id)}`}>{avatar.name}</a>
              <button
                className="chip-remove"
                type="button"
                aria-label={`${avatar.name} の結び付けを外す`}
                title="結び付けを外す"
                disabled={busy}
                onClick={() => onLink(avatar.id, false)}
              >
                ×
              </button>
            </span>
          ))}
          {linkable.length > 0 && (
            <select
              className="project-avatar-add"
              aria-label={`${project.name} にアバターを結び付ける`}
              value=""
              disabled={busy}
              onChange={(event) => {
                if (event.target.value) onLink(event.target.value, true);
              }}
            >
              <option value="">{project.avatars.length === 0 ? "アバターを結び付ける…" : "アバターを足す…"}</option>
              {linkable.map((avatar) => (
                <option key={avatar.id} value={avatar.id}>
                  {avatar.displayName}
                </option>
              ))}
            </select>
          )}
        </div>
      </div>
      <div className="project-actions">
        <button className="button primary" type="button" disabled={!canOpenProject(project) || opening} onClick={onOpen}>
          開く
        </button>
        <button className="button secondary" type="button" disabled={project.state === "missing"} onClick={onReveal}>
          フォルダを表示
        </button>
        {project.source === "alcom" ? (
          <button className="button secondary" type="button" disabled={busy} onClick={() => onHide(!project.hidden)}>
            {project.hidden ? "一覧に戻す" : "一覧から隠す"}
          </button>
        ) : (
          <button className="button secondary" type="button" disabled={busy} onClick={onRemove}>
            一覧から消す
          </button>
        )}
      </div>
    </li>
  );
}
