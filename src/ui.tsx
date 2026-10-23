import { useState } from "react";
import { FILE_PRESENCE_LABEL, mediaSrc } from "./media";
import type { FilePresence } from "./media";

export function Thumb({
  path,
  url,
  name,
  small = false,
}: {
  path: string | null;
  url: string | null;
  name: string;
  small?: boolean;
}) {
  const [failed, setFailed] = useState(false);
  const src = failed ? null : mediaSrc(path, url);
  const letter = name.trim().slice(0, 1) || "?";
  if (!src) {
    return <div className={small ? "thumb placeholder small" : "thumb placeholder"}>{letter}</div>;
  }
  return (
    <img
      className={small ? "thumb small" : "thumb"}
      src={src}
      alt=""
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
    />
  );
}

export function FileMark({
  presence,
  href,
  onClick,
}: {
  presence: FilePresence;
  href?: string;
  onClick?: () => void;
}) {
  const className = `presence presence-${presence}`;
  const label = FILE_PRESENCE_LABEL[presence];
  if (href) {
    return (
      <a className={className} href={href}>
        {label}
      </a>
    );
  }
  if (onClick) {
    return (
      <button className={className} type="button" onClick={onClick}>
        {label}
      </button>
    );
  }
  return <span className={className}>{label}</span>;
}

export function errorText(err: unknown): string {
  if (typeof err === "string" && err.trim()) return err;
  if (err instanceof Error && err.message) return err.message;
  return "操作に失敗しました";
}
