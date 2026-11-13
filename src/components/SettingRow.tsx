import type { ReactNode } from "react";

export function SettingRow({
  id,
  label,
  help,
  note,
  children,
}: {
  /** 検索の結果から移る先（`src/appSearchIndex.ts`）。 */
  id?: string;
  label: string;
  help?: ReactNode;
  /** 操作の結果や前提条件。help より大きく濃く出す（.note-strong）。 */
  note?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="setting-row" id={id}>
      <div className="setting-copy">
        <strong>{label}</strong>
        {help && <p>{help}</p>}
        {note && <p className="note-strong">{note}</p>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}
