import type { ReactNode } from "react";
import { openSettingsSection, type SettingsSectionId } from "../anchorScroll";

/** 設定の節へのリンク。設定画面の先頭ではなく、その節まで移る（`openSettingsSection`）。 */
export function SettingsLink({
  section,
  className = "button secondary",
  children,
}: {
  section: SettingsSectionId;
  className?: string;
  children: ReactNode;
}) {
  return (
    <a
      className={className}
      href="#/settings"
      onClick={(event) => {
        event.preventDefault();
        openSettingsSection(section);
      }}
    >
      {children}
    </a>
  );
}
