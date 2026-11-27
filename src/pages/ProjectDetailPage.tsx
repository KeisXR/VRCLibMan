import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, onUnityIndexChanged } from "../api";
import { useShell } from "../App";
import { useOpenUnityProject, useUnityProjects } from "../components/UnityProjects";
import { mediaSrc } from "../media";
import type { ProjectContents, ProjectProduct } from "../types";
import { errorText } from "../ui";
import {
  PROJECT_KIND_LABEL,
  VERDICT_LABEL,
  needsScan,
  projectProductHref,
  projectSections,
  projectStateNote,
  scannedLabel,
} from "../unityProjects";

/** プロジェクトの詳細（docs/unity-projects.md の段階 2）。入っている商品を GUID の一致で見せる。読むだけ。 */
export function ProjectDetailPage({ id }: { id: string }) {
  const { fail } = useShell();
  const { list } = useUnityProjects();
  const { open, opening } = useOpenUnityProject();
  const project = list?.projects.find((item) => item.id === id) ?? null;
  const [contents, setContents] = useState<ProjectContents | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const autoScanned = useRef<string | null>(null);

  // いま表示しているプロジェクト。調べている間に別のプロジェクトへ移ったら、遅れて届いた結果を捨てる。
  const shown = useRef(id);
  shown.current = id;

  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      const value = await api.scanUnityProject(id);
      if (shown.current === id) setContents(value);
    } catch (err) {
      if (shown.current === id) setError(errorText(err));
    } finally {
      if (shown.current === id) setScanning(false);
    }
  }, [id]);

  useEffect(() => {
    let cancelled = false;
    setContents(null);
    setError(null);
    setScanning(false);
    const load = () =>
      api.unityProjectContents(id).then(
        (value) => {
          if (!cancelled) setContents(value);
        },
        (err: unknown) => {
          if (!cancelled) fail(errorText(err));
        },
      );
    void load();
    // ライブラリの索引が変わった（裏でファイルを読んだ）ら、結果を読み直す。調べ直しはしない。
    const stop = onUnityIndexChanged(() => {
      if (!cancelled) void load();
    });
    return () => {
      cancelled = true;
      stop();
    };
  }, [id, fail]);

  // 結果が無いか古ければ、開いたときに 1 回だけ調べる。
  useEffect(() => {
    if (!contents || !project || project.state !== "ok" || autoScanned.current === id) return;
    if (needsScan(contents)) {
      autoScanned.current = id;
      void scan();
    }
  }, [contents, project, id, scan]);

  const sections = useMemo(() => (contents ? projectSections(contents) : null), [contents]);

  if (list && !project) {
    return (
      <section>
        <p><a href="#/projects">プロジェクト一覧へ</a></p>
        <div className="empty"><p>プロジェクトが見つかりません。一覧から隠したか、ALCOM の一覧から消えた可能性があります。</p></div>
      </section>
    );
  }
  const note = project ? projectStateNote(project) : null;

  return (
    <section className="project-detail">
      <p><a href="#/projects">プロジェクト一覧へ</a></p>
      <div className="page-head">
        <div>
          <h1>{project?.name ?? "プロジェクト"}</h1>
          {project && (
            <p>
              {[PROJECT_KIND_LABEL[project.kind], project.version && `Unity ${project.version}`, note].filter(Boolean).join("・")}
              {project.avatars.length > 0 && <>・{project.avatars.map((avatar, index) => (
                <span key={avatar.id}>{index > 0 && "、"}<a href={`#/avatar/${encodeURIComponent(avatar.id)}`}>{avatar.name}</a></span>
              ))}</>}
            </p>
          )}
        </div>
        {project && (
          <div className="button-row">
            <button className="button primary" type="button" disabled={project.state !== "ok" || opening === project.id} onClick={() => void open(project)}>
              {project.running === "open" ? "開いています" : "開く"}
            </button>
            <button className="button secondary" type="button" disabled={scanning || project.state !== "ok"} onClick={() => void scan()}>
              {scanning ? "調べています…" : "調べ直す"}
            </button>
          </div>
        )}
      </div>

      <p className="meta project-scan-note" role="status">
        {scanning
          ? "ライブラリの Unity パッケージと、このプロジェクトの Assets を照らし合わせています。プロジェクトの中身は書き換えません。"
          : contents
            ? `${scannedLabel(contents.scannedAt)}${contents.stale ? "（そのあとライブラリが変わりました）" : ""}`
            : "読み込んでいます…"}
      </p>
      {error && <p className="note note-strong">{error}</p>}
      {contents && contents.scannedAt != null && !contents.complete && (
        <p className="note note-strong">プロジェクトが大きいため、途中までしか調べられませんでした。ここに無い商品も、入っていないとは限りません。</p>
      )}

      {sections && contents?.scannedAt != null && (
        <>
          <ProductSection title="入っている商品" products={sections.installed} empty="ライブラリの商品で、このプロジェクトに入っていると分かったものはありません。" />
          {sections.partial.length > 0 && (
            <ProductSection
              title="一部が入っています"
              products={sections.partial}
              lede="パッケージのファイルの一部だけが見つかりました。取り込んだあとに消したか、ほかの商品と同じファイルを使っている可能性があります。"
            />
          )}
          {sections.bundled.length > 0 && (
            <details className="generic project-section">
              <summary>同梱されていた共通パッケージ（{sections.bundled.length}）</summary>
              <p className="meta">
                商品の zip に入っていた lilToon などの共通パッケージです。同じものを ALCOM で入れているプロジェクトでは、取り込む必要はありません。
              </p>
              <ProductList products={sections.bundled} />
            </details>
          )}
          {sections.unknown.length > 0 && (
            <details className="generic project-section">
              <summary>判定できない商品（{sections.unknown.length}）</summary>
              <p className="meta">この商品だけのファイルが無いため（ほかの商品と同じファイルだけ、など）、入っているかを判定できません。</p>
              <ProductList products={sections.unknown} />
            </details>
          )}
        </>
      )}
    </section>
  );
}

function ProductSection({ title, products, empty, lede }: { title: string; products: ProjectProduct[]; empty?: string; lede?: string }) {
  return (
    <div className="project-section">
      <h2>
        {title}
        <span className="tab-count">{products.length}</span>
      </h2>
      {lede && <p className="meta">{lede}</p>}
      {products.length === 0 && empty ? <p className="meta">{empty}</p> : <ProductList products={products} />}
    </div>
  );
}

function ProductList({ products }: { products: ProjectProduct[] }) {
  return (
    <ul className="project-products">
      {products.map((product) => {
        const href = projectProductHref(product);
        const src = mediaSrc(product.thumbnailPath, product.thumbnailUrl);
        const shown = product.packages.filter((item) => item.verdict !== "missing" || product.verdict === "missing");
        return (
          <li key={product.boothId} className="project-product">
            {src ? <img className="project-product-thumb" src={src} alt="" loading="lazy" /> : <span className="project-product-thumb" aria-hidden="true" />}
            <div className="project-product-main">
              {href ? <a href={href}>{product.title}</a> : <span>{product.title}</span>}
              <ul className="project-packages">
                {shown.map((item) => (
                  <li key={`${item.sourcePath}:${item.name}`} className="meta">
                    {item.name}
                    {" — "}
                    {item.verdict === "bundled"
                      ? `${item.bundledWith ?? "VPM パッケージ"} の同梱版`
                      : item.uniqueTotal > 0
                        ? `${VERDICT_LABEL[item.verdict]}（${item.uniqueMatched} / ${item.uniqueTotal} ファイル）`
                        : VERDICT_LABEL[item.verdict]}
                    {item.closest && <span className="badge project-closest">入っている版の見込み</span>}
                  </li>
                ))}
              </ul>
            </div>
          </li>
        );
      })}
    </ul>
  );
}
