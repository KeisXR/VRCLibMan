import { GenreBrowser } from "../components/GenreBrowser";

export function WorldsPage({ genre }: { genre?: string }) {
  return (
    <GenreBrowser
      kind="world"
      genre={genre ?? null}
      basePath="#/worlds"
      title="ワールド"
      lede="ワールド本体、家具・小物、ギミック、テクスチャ置換、Skybox などをジャンルごとに見られます。"
    />
  );
}
