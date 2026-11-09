import { useCallback } from "react";
import { api } from "./api";
import { useShell } from "./App";
import { GENRE_LABEL } from "./types";
import type { GenreKind } from "./types";
import { errorText } from "./ui";

export function useGenreMove() {
  const { refresh, notify, fail } = useShell();
  return useCallback(
    async (boothId: string, kind: GenreKind, genre: string) => {
      try {
        const snapshot = await api.moveProductGenre(boothId, kind, genre);
        notify(`${GENRE_LABEL[genre] ?? genre}へ移動しました`, () => {
          void api.restoreProductClassification(snapshot).then(
            () => {
              notify("移動を元に戻しました");
              refresh();
            },
            (err: unknown) => fail(errorText(err)),
          );
        });
        refresh();
      } catch (err) {
        fail(errorText(err));
      }
    },
    [fail, notify, refresh],
  );
}

export function useRegisterOwnedBody() {
  const { refresh, notify, fail } = useShell();
  return useCallback(
    async (boothId: string, name?: string) => {
      try {
        const result = await api.registerOwnedBody(boothId, name);
        if (result.kind === "registered") {
          const label = result.displayName;
          notify(`${label}をアバター本体として登録しました`, () => {
            void api.restoreProductClassification(result.snapshot).then(
              () => {
                notify("登録を元に戻しました");
                refresh();
              },
              (err: unknown) => fail(errorText(err)),
            );
          });
          refresh();
        }
        return result;
      } catch (err) {
        fail(errorText(err));
        return null;
      }
    },
    [fail, notify, refresh],
  );
}

export function useUnpinProduct() {
  const { refresh, notify, fail } = useShell();
  return useCallback(
    async (boothId: string) => {
      try {
        const snapshot = await api.unpinProductClassification(boothId);
        notify("自動分類の対象に戻しました", () => {
          void api.restoreProductClassification(snapshot).then(
            () => {
              notify("手動の分類に戻しました");
              refresh();
            },
            (err: unknown) => fail(errorText(err)),
          );
        });
        refresh();
      } catch (err) {
        fail(errorText(err));
      }
    },
    [fail, notify, refresh],
  );
}
