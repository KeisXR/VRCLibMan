import { useEffect, useRef, useState } from "react";
import { api, chooseHeroImage } from "../api";
import { useShell } from "../App";
import { errorText } from "../ui";

/** ヒーロー右上の画像メニュー。操作行には置かない。 */
export function AvatarHeroMenu({
  avatarId,
  avatarName,
  hasCustomImage,
}: {
  avatarId: string;
  avatarName: string;
  hasCustomImage: boolean;
}) {
  const { refresh, notify, fail } = useShell();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!container.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        trigger.current?.focus();
      }
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  async function run(work: () => Promise<string | null>) {
    setOpen(false);
    trigger.current?.focus();
    setBusy(true);
    try {
      const message = await work();
      if (message) {
        notify(message);
        refresh();
      }
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  const change = () =>
    run(async () => {
      const path = await chooseHeroImage();
      if (!path) return null;
      await api.setAvatarHeroImage(avatarId, path);
      return "ヒーロー画像を変更しました";
    });

  const clear = () =>
    run(async () => {
      await api.clearAvatarHeroImage(avatarId);
      return "ヒーロー画像をサムネイルに戻しました";
    });

  return (
    <div
      className="product-actions"
      ref={container}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
      }}
    >
      <button
        ref={trigger}
        className="icon-button avatar-hero-menu-trigger"
        type="button"
        aria-label={`${avatarName}のヒーロー画像の操作`}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-busy={busy}
        title="ヒーロー画像"
        onClick={() => {
          if (!busy) setOpen((value) => !value);
        }}
      >
        <span aria-hidden="true">⋯</span>
      </button>
      {open && (
        <div className="product-actions-panel" role="menu">
          <button className="button" type="button" role="menuitem" onClick={() => void change()}>
            画像を変更
          </button>
          <button
            className="button"
            type="button"
            role="menuitem"
            disabled={!hasCustomImage}
            onClick={() => void clear()}
          >
            サムネに戻す
          </button>
        </div>
      )}
    </div>
  );
}
