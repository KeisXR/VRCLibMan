import { GENRE_FREE, GENRE_LABEL, GENRE_UNSET, genresForKind } from "../types";
import type { GenreKind } from "../types";
import { useGenreBrowse } from "../genreBrowseContext";
import { genreHref } from "../genreBrowseUtils";
import { ChipBar } from "./ChipBar";
import { GenreTarget } from "./GenreColumn";

const CHIP = { className: "chip-bar-item", labelClassName: "chip-bar-label", countClassName: "chip-bar-count" };

/**
 * The genre column as a one-line strip above the list, for windows where the column would
 * leave the list less than 480px. Same links, counts and drop targets as the column.
 */
export function GenreChipBar() {
  const { kind, basePath, selected, data, draggingId, setDraggingId, moveProduct } = useGenreBrowse();
  const unsetCount = data?.counts[GENRE_UNSET] ?? 0;
  const across: GenreKind = kind === "world" ? "asset" : "world";
  const drop = (targetKind: GenreKind, genre: string) => (boothId: string) => {
    setDraggingId(null);
    void moveProduct(boothId, targetKind, genre);
  };

  return (
    <ChipBar label="ジャンル" activeKey={selected ?? ""} className="genre-chip-bar">
      <GenreTarget
        {...CHIP}
        href={genreHref(basePath, null)}
        label="すべて"
        count={data?.total}
        active={selected === null}
        onDropId={() => undefined}
      />
      {genresForKind(kind).map((value) => (
        <GenreTarget
          {...CHIP}
          key={value}
          href={genreHref(basePath, value)}
          label={GENRE_LABEL[value] ?? value}
          count={data ? data.counts[value] ?? 0 : undefined}
          active={selected === value}
          dropGenre={value}
          onDropId={drop(kind, value)}
        />
      ))}
      <GenreTarget
        {...CHIP}
        href={genreHref(basePath, GENRE_FREE)}
        label={GENRE_LABEL[GENRE_FREE]}
        count={data ? data.counts[GENRE_FREE] ?? 0 : undefined}
        active={selected === GENRE_FREE}
        onDropId={() => undefined}
      />
      {(unsetCount > 0 || selected === GENRE_UNSET) && (
        <GenreTarget
          {...CHIP}
          href={genreHref(basePath, GENRE_UNSET)}
          label={GENRE_LABEL[GENRE_UNSET]}
          count={data ? unsetCount : undefined}
          active={selected === GENRE_UNSET}
          onDropId={() => undefined}
        />
      )}
      {draggingId && (
        <>
          <span className="chip-bar-sep" aria-hidden="true" />
          <span className="chip-bar-kicker">{across === "world" ? "ワールド" : "アセット"}</span>
          {genresForKind(across).map((value) => (
            <GenreTarget
              {...CHIP}
              key={`${across}-${value}`}
              label={GENRE_LABEL[value] ?? value}
              dropGenre={value}
              onDropId={drop(across, value)}
            />
          ))}
        </>
      )}
    </ChipBar>
  );
}
