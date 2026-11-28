import type { InstallVerdict, ProjectContents, ProjectProduct, UnityProject } from "./types";

/** 判断だけの補助。業務の判定（開けるか、どの Unity か）は Rust の `unity_projects.rs` が持つ。 */

export const PROJECT_KIND_LABEL: Record<UnityProject["kind"], string> = {
  avatar: "アバター",
  world: "ワールド",
  unknown: "種類不明",
};

/** 名前・パス・結び付けたアバターで絞る。空白区切りはすべてを含むもの。 */
export function filterProjects(projects: UnityProject[], query: string, showHidden: boolean): UnityProject[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return projects.filter((project) => {
    if (project.hidden && !showHidden) return false;
    if (words.length === 0) return true;
    const text = [project.name, project.path, ...project.avatars.map((avatar) => avatar.name)].join("\n").toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

/** プロジェクトの詳細（中身）の画面。 */
export function projectDetailHref(projectId: string): string {
  return `#/projects/${encodeURIComponent(projectId)}`;
}

/** 一覧の行に添える、いまの状態の短い言葉。無ければ null。 */
export function projectStateNote(project: UnityProject): string | null {
  if (project.state === "missing") return "フォルダが見つかりません";
  if (project.state === "timeout") return "確認できません";
  if (project.running === "open") return "開いています";
  if (project.running === "maybe") return "開いているかもしれません";
  if (!project.editorAvailable && project.version) return `Unity ${project.version} が見つかりません`;
  return null;
}

/** 「開く」を押せるか。版の合う Unity が無いときも押せるようにし、押したときに理由を出す。 */
export function canOpenProject(project: UnityProject): boolean {
  return project.state === "ok";
}

/** 最後に触った日。今日・昨日・n 日前、それより前は日付。 */
export function modifiedLabel(modified: number | null, now: number = Date.now()): string {
  if (modified == null) return "";
  const days = Math.floor((now / 1000 - modified) / 86400);
  if (days <= 0) return "今日";
  if (days === 1) return "昨日";
  if (days < 30) return `${days} 日前`;
  const date = new Date(modified * 1000);
  return `${date.getFullYear()}/${date.getMonth() + 1}/${date.getDate()}`;
}

export const VERDICT_LABEL: Record<InstallVerdict, string> = {
  installed: "導入済み",
  partial: "一部が入っています",
  missing: "入っていません",
  unknown: "判定できません",
  bundled: "同梱版",
};

/**
 * 入っている版の知らせ。判定は Rust（`unity_index::version_report`）で、ここは言い方だけ。
 * `note` は版について言えること、`adjusted` はマテリアルなどの違い（改変とは言わずに数だけ）。
 */
export function versionLines(product: Pick<ProjectProduct, "version" | "adjusted">): { note: string | null; adjusted: string | null } {
  const version = product.version;
  let note: string | null = null;
  if (version?.kind === "older") {
    note = `古い版が入っています。新しいファイル（${version.newer}）があります`;
  } else if (version?.kind === "differs") {
    note =
      version.versions >= 2
        ? `手元のどの版とも違うテクスチャ・モデルなどが ${version.files} 個あります。改変したか、手元に無い版です`
        : `手元のファイルと違うテクスチャ・モデルなどが ${version.files} 個あります。古い版か、改変した可能性があります`;
  }
  const adjusted =
    product.adjusted > 0
      ? `マテリアル・プレハブなど ${product.adjusted} 個は配布物から変わっています（色や設定の調整、Unity の保存し直しでも変わります）`
      : null;
  return { note, adjusted };
}

/** プロジェクトの詳細の節。入っていないものは並べない（ライブラリ全部が並ぶので）。 */
export interface ProjectSections {
  installed: ProjectProduct[];
  partial: ProjectProduct[];
  unknown: ProjectProduct[];
  /** 同梱版だけの商品（商品の zip に入っていた lilToon など）。 */
  bundled: ProjectProduct[];
}

export function projectSections(contents: ProjectContents): ProjectSections {
  const by = (verdict: InstallVerdict) =>
    contents.products
      .filter((product) => product.verdict === verdict)
      .sort((a, b) => a.title.localeCompare(b.title, "ja"));
  return { installed: by("installed"), partial: by("partial"), unknown: by("unknown"), bundled: by("bundled") };
}

/** 商品をアプリの中で開く先。購入履歴と同じ形（Rust の `library_link`）。 */
export function projectProductHref(product: ProjectProduct): string | null {
  const link = product.link;
  if (!link) return null;
  if (link.kind === "avatar") return link.avatarId ? `#/avatar/${encodeURIComponent(link.avatarId)}` : null;
  return `#/${link.kind === "world" ? "worlds" : "assets"}/item/${encodeURIComponent(product.boothId)}`;
}

/** 調べた日時の言い方。 */
export function scannedLabel(scannedAt: number | null, now: number = Date.now()): string {
  if (scannedAt == null) return "まだ調べていません";
  const minutes = Math.floor((now / 1000 - scannedAt) / 60);
  if (minutes < 1) return "いま調べました";
  if (minutes < 60) return `${minutes} 分前に調べました`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 時間前に調べました`;
  return `${modifiedLabel(scannedAt, now)}に調べました`;
}

/** 開いたときに調べ直すか。結果が無い・古いとき。 */
export function needsScan(contents: ProjectContents | null): boolean {
  return contents == null || contents.scannedAt == null || contents.stale;
}

/** 商品カードの「導入済み」の印に使う、商品ごとの判定（導入済みと一部だけ）。 */
export function installMarks(contents: ProjectContents | null): Map<string, "installed" | "partial"> {
  const marks = new Map<string, "installed" | "partial">();
  // 調べていない・調べたあとに索引が変わった（古い）結果からは印を出さない。
  if (!contents || contents.scannedAt == null || contents.stale) return marks;
  for (const product of contents.products) {
    if (product.verdict === "installed" || product.verdict === "partial") marks.set(product.boothId, product.verdict);
  }
  return marks;
}

/** 印を出すプロジェクト。選んでいたものが結び付けに残っていればそれ、無ければ最初の 1 つ。 */
export function markProjectId(linked: UnityProject[], current: string | null): string | null {
  if (current && linked.some((project) => project.id === current)) return current;
  return linked[0]?.id ?? null;
}

/** 入っているアバター本体のうち、プロジェクトにまだ結び付けていないもの（結び付けの候補）。 */
export function bodySuggestions(contents: ProjectContents | null, project: UnityProject | null): ProjectProduct[] {
  // 古い結果からは言わない（調べ直すまで何も言わない）。
  if (!contents || !project || contents.scannedAt == null || contents.stale) return [];
  const linked = new Set(project.avatars.map((avatar) => avatar.id));
  return contents.products.filter(
    (product) =>
      product.verdict === "installed" && product.link?.kind === "avatar" && product.link.avatarId != null && !linked.has(product.link.avatarId),
  );
}

/** アバターに結び付けられるプロジェクト（隠しておらず、ワールドのプロジェクトでなく、まだ結び付けていないもの）。 */
export function linkableProjects(projects: UnityProject[], avatarId: string): UnityProject[] {
  return projects.filter(
    (project) => !project.hidden && project.kind !== "world" && !project.avatars.some((avatar) => avatar.id === avatarId),
  );
}

/** VPM パッケージの見せ方。読みやすい名前を知っていれば「lilToon（jp.lilxyzw.liltoon）」、知らなければ名前そのもの。 */
export function vpmPackageLabel(name: string, names: Record<string, string> | undefined): string {
  const display = names?.[name]?.trim();
  return display && display !== name ? `${display}（${name}）` : name;
}
