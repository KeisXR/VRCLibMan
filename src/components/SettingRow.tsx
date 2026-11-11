import type { ReactNode } from "react";

export function SettingRow({
  label,
  help,
  note,
  children,
}: {
  label: string;
  help?: ReactNode;
  /** 操作の結果や前提条件。help より大きく濃く出す（.note-strong）。 */
  note?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="setting-row">
      <div className="setting-copy">
        <strong>{label}</strong>
        {help && <p>{help}</p>}
        {note && <p className="note-strong">{note}</p>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}
