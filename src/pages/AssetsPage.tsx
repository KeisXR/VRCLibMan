import { GenreBrowser } from "../components/GenreBrowser";

export function AssetsPage({ genre }: { genre?: string }) {
  return (
    <GenreBrowser
      kind="asset"
      genre={genre ?? null}
      basePath="#/assets"
      title="アセット"
      lede="アバターとは別に、改変用アセットをジャンルごとに見られます。"
    />
  );
}
