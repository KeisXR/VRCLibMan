import { othersLabel, type ProgressStrip } from "../progress";
import { topbarMotionEnabled, useExitPresence, useTopbarMotion } from "../topbarMotion";
import { ProgressTrack } from "../ui";

/**
 * 上部の進捗の帯。出るときは下の内容を滑らかに押し下げ、仕事が無くなったら最後の表示のまま薄れて詰まる
 * （速さは左サイドバーの開閉と同じ。`src/topbarMotion.ts`）。
 */
export function ProgressStripBar({ strip }: { strip: ProgressStrip | null }) {
  const shown = useExitPresence(strip, topbarMotionEnabled());
  if (!shown.value) return null;
  return <StripBody strip={shown.value} leaving={shown.leaving} onLeft={shown.onLeft} />;
}

function StripBody({ strip, leaving, onLeft }: { strip: ProgressStrip; leaving: boolean; onLeft: () => void }) {
  const motion = useTopbarMotion(leaving, onLeft);
  return (
    <div {...motion} className="progress">
      {strip.others > 0 ? (
        <div className="progress-head">
          <p>{strip.entry.message}</p>
          <span className="progress-more">{othersLabel(strip.others)}</span>
        </div>
      ) : (
        <p>{strip.entry.message}</p>
      )}
      <ProgressTrack current={strip.entry.current} total={strip.entry.total} state={strip.entry.state} label={strip.entry.message} />
    </div>
  );
}
