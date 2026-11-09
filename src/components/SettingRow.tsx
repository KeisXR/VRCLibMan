import type { ReactNode } from "react";

export function SettingRow({
  label,
  help,
  children,
}: {
  label: string;
  help?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="setting-row">
      <div className="setting-copy">
        <strong>{label}</strong>
        {help && <p>{help}</p>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}
