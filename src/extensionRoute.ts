import { useEffect } from "react";
import { api, isTauri } from "./api";

/**
 * 拡張の「アプリで開く」で届く行き先。Rust が登録済みの商品から組み立てるが、画面でも形を確かめ、
 * アセット・ワールドの商品とアバターの画面以外へは移らない。
 */
export function extensionRoute(value: unknown): string | null {
  if (typeof value !== "string") return null;
  return /^#\/(?:assets|worlds)\/item\/\d{4,12}$/.test(value) || /^#\/avatar\/[^/#?\s]+$/.test(value) ? value : null;
}

/** 起動した直後と、頼まれたときに、行き先を 1 回だけ受け取ってその画面へ移る。 */
export function useExtensionRoute(): void {
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    let stop = () => undefined as void;
    const take = () => {
      api.takeExtensionRoute().then(
        (value) => {
          const route = extensionRoute(value);
          if (alive && route) window.location.hash = route;
        },
        () => undefined,
      );
    };
    take();
    import("@tauri-apps/api/event")
      .then(({ listen }) => listen("extension-open-product", take))
      .then((unlisten) => {
        if (alive) stop = unlisten;
        else unlisten();
      });
    return () => {
      alive = false;
      stop();
    };
  }, []);
}
