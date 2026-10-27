import { useCallback, useRef } from "react";
import { UI_RESIZE_STEP_PX } from "../uiLayout";

export function ResizeSeparator({
  label,
  value,
  min,
  max,
  onChange,
  onCommit,
  onReset,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (next: number) => void;
  onCommit: (next: number) => void;
  onReset: () => void;
}) {
  const dragging = useRef(false);
  const startX = useRef(0);
  const startValue = useRef(value);

  const finish = useCallback(
    (next: number) => {
      dragging.current = false;
      document.body.style.removeProperty("user-select");
      onCommit(next);
    },
    [onCommit],
  );

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragging.current = true;
    startX.current = event.clientX;
    startValue.current = value;
    document.body.style.userSelect = "none";
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    const delta = event.clientX - startX.current;
    const next = Math.min(max, Math.max(min, startValue.current + delta));
    onChange(next);
  };

  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    event.currentTarget.releasePointerCapture(event.pointerId);
    finish(Math.min(max, Math.max(min, startValue.current + (event.clientX - startX.current))));
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      const next = Math.max(min, value - UI_RESIZE_STEP_PX);
      onChange(next);
      onCommit(next);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      const next = Math.min(max, value + UI_RESIZE_STEP_PX);
      onChange(next);
      onCommit(next);
    } else if (event.key === "Home") {
      event.preventDefault();
      onReset();
    }
  };

  return (
    <div
      className="resize-separator"
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={Math.round(value)}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={() => onReset()}
      onKeyDown={onKeyDown}
    />
  );
}
