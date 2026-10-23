export type Category = "avatar" | "outfit" | "hair" | "accessory" | "texture" | "gimmick" | "world" | "other";

export const CATEGORY_LABEL: Record<string, string> = {
  avatar: "アバター",
  outfit: "衣装",
  hair: "髪",
  accessory: "アクセサリ",
  texture: "テクスチャ",
  gimmick: "ギミック",
  world: "ワールド",
  other: "その他",
  uncategorized: "未分類",
};

export const CATEGORIES: Category[] = ["avatar", "outfit", "hair", "accessory", "texture", "gimmick", "world", "other"];

export interface AvatarCard {
  id: string;
  displayName: string;
  baseBody: string | null;
  baseBodyName: string | null;
  aliases: string[];
  isBuiltin: boolean;
  isPlaceholder: boolean;
  owned: boolean;
  thumbnailPath: string | null;
  thumbnailUrl: string | null;
  outfitCount: number;
  hairCount: number;
  gimmickCount: number;
  genericCount: number;
}

export interface FileBrief {
  path: string;
  filename: string;
  isDir: boolean;
  size: number;
  matchScore: number;
}

export interface ItemView {
  boothId: string;
  title: string;
  shop: string;
  url: string;
  descriptionSnippet: string;
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
  category: Category | null;
  isGeneric: boolean;
  confidence: number | null;
  viaBaseBody: boolean;
  localFiles: FileBrief[];
  localFileCount: number;
}

export interface OwnedBody {
  boothId: string;
  title: string;
  shop: string;
  url: string;
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
}

export interface Section {
  category: string;
  items: ItemView[];
}

export interface Sibling {
  id: string;
  displayName: string;
}

export interface AvatarDetail {
  id: string;
  displayName: string;
  baseBody: string | null;
  baseBodyName: string | null;
  baseBodyNote: string | null;
  aliases: string[];
  isBuiltin: boolean;
  isPlaceholder: boolean;
  owned: boolean;
  siblings: Sibling[];
  ownedProducts: OwnedBody[];
  sections: Section[];
  genericItems: ItemView[];
}

export interface QueueProduct {
  boothId: string;
  title: string;
  shop: string;
  url: string;
  descriptionSnippet: string;
  thumbnailUrl: string | null;
  thumbnailPath: string | null;
  category: Category | null;
  isGeneric: boolean;
  confidence: number | null;
  compatibleAvatars: string[];
  unresolvedNames: string[];
  linkedAvatarIds: string[];
  userConfirmed: boolean;
}

export interface QueueFile {
  path: string;
  filename: string;
  isDir: boolean;
  size: number;
  matchScore: number;
  candidateProductId: string | null;
  candidateTitle: string | null;
}

export interface QueuePage {
  unextracted: number;
  needsReview: number;
  unlinkedFiles: number;
  products: QueueProduct[];
  files: QueueFile[];
}

export interface ProductOption {
  boothId: string;
  title: string;
  shop: string;
}

export interface AvatarRecord {
  id: string;
  displayName: string;
  baseBody: string | null;
  aliases: string[];
  isBuiltin: boolean;
  owned: boolean;
  notes: string;
}

export interface BaseBody {
  id: string;
  displayName: string;
  note: string;
}

export interface Knowledge {
  avatars: AvatarRecord[];
  baseBodies: BaseBody[];
}

export interface ExtensionLink {
  token: string;
  port: number;
  listening: boolean;
  error: string | null;
}

export interface Settings {
  ollamaBaseUrl: string;
  ollamaModel: string;
  confidenceThreshold: number;
  libraryFolder: string;
  builtinBrowser: boolean;
}

export interface LibraryStats {
  products: number;
  ownedAvatars: number;
  unextracted: number;
  needsReview: number;
  queueCount: number;
  unlinkedFiles: number;
  ollamaModel: string;
}

export interface ImportReport {
  inserted: number;
  updated: number;
  unchanged: number;
  skipped: number;
  warnings: string[];
}

export interface ScanReport {
  indexed: number;
  linked: number;
  truncated: boolean;
  root: string;
}

export interface ExtractReport {
  processed: number;
  failed: number;
  cancelled: boolean;
  errors: string[];
}

export interface OllamaStatus {
  models: string[];
  modelReady: boolean;
}

export interface CatalogInfo {
  count: number;
  fetchedOn: string;
  upstreamUpdated: string;
  sourceUrl: string;
  termsUrl: string;
  author: string;
  credit: string;
}

export interface CatalogRefresh {
  added: number;
  total: number;
  upstreamUpdated: string;
}

export interface SetupStatus {
  complete: boolean;
  model: string;
  modelSizeHint: string;
}

export interface AvatarCandidate {
  name: string;
  productCount: number;
}

export interface JobProgress {
  job: string;
  current: number;
  total: number;
  message: string;
}

export interface ProductUpdate {
  boothId: string;
  category?: string | null;
  isGeneric?: boolean | null;
  avatarIds?: string[] | null;
  confirm?: boolean | null;
}

export interface AvatarInput {
  id?: string | null;
  displayName: string;
  baseBody?: string | null;
  aliases: string[];
  notes: string;
}
