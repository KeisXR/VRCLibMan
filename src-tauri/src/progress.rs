//! 進捗（`job-progress`）の送り方を1か所にまとめる。
//!
//! 決まり（docs/design/README.md#進捗）:
//! - 始めた仕事は、成功でも失敗でも途中の `?` でも、必ず終わりの状態（done / error / cancelled）を送る。
//!   `ProgressGuard` は Drop で送り漏れを防ぐ。
//! - メッセージには件数だけを書き、商品名やクエリは書かない。
//! - `background` の仕事は上部の帯に出さず、その設定の欄だけで見せる。

use crate::model::{JobProgress, ProgressState};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

pub const EVENT: &str = "job-progress";

/// 途中で抜けた（Drop まで終わりを送らなかった）ときの文言。
pub const INTERRUPTED_MESSAGE: &str = "中断しました";

/// 進捗の送り先。アプリでは `AppHandle`、テストでは記録用の入れ物。
pub trait ProgressSink {
    fn send(&self, progress: JobProgress);
}

impl ProgressSink for AppHandle {
    fn send(&self, progress: JobProgress) {
        let _ = self.emit(EVENT, progress);
    }
}

/// 状態を明示しない古い送り方の推定。`total > 0` で `current >= total` なら終わり。
pub fn inferred_state(current: u32, total: u32) -> ProgressState {
    if total > 0 && current >= total {
        ProgressState::Done
    } else {
        ProgressState::Running
    }
}

/// 状態を明示せずに送る。終わりの判定は [`inferred_state`] に任せる。
/// 失敗や 0 件で終わる仕事は、これではなく [`ProgressGuard`] を使う。
pub fn emit_progress(app: &AppHandle, job: &str, current: u32, total: u32, message: impl Into<String>) {
    app.send(JobProgress {
        job: job.to_string(),
        current,
        total,
        message: message.into(),
        state: inferred_state(current, total),
        background: false,
    });
}

/// 仕事の終わりの1件を作る。`outcome` が `None` なら、終わりを送らずに抜けたので error にする。
/// done は `current = total` に揃える（0 件なら 0 / 0 のまま done）。
/// error と cancelled は最後に送った数を残す。
pub fn final_progress(
    job: &str,
    background: bool,
    last_current: u32,
    last_total: u32,
    outcome: Option<(ProgressState, String)>,
) -> JobProgress {
    let (state, message) = match outcome {
        Some((ProgressState::Running, message)) => (ProgressState::Done, message),
        Some((state, message)) => (state, message),
        None => (ProgressState::Error, INTERRUPTED_MESSAGE.to_string()),
    };
    let (current, total) = match state {
        ProgressState::Done => (last_total, last_total),
        _ => (last_current, last_total),
    };
    JobProgress {
        job: job.to_string(),
        current,
        total,
        message,
        state,
        background,
    }
}

struct Last {
    current: u32,
    total: u32,
    finished: bool,
}

/// 1つの仕事の進捗。`update` で途中を送り、`done` / `fail` / `cancel` で終わりを送る。
/// どれも呼ばずに Drop されたら（`?` で抜けた、panic した）error を送る。
pub struct ProgressGuard<S: ProgressSink = AppHandle> {
    sink: S,
    job: String,
    background: bool,
    last: Mutex<Last>,
}

impl<S: ProgressSink> ProgressGuard<S> {
    pub fn new(sink: S, job: &str) -> Self {
        Self {
            sink,
            job: job.to_string(),
            background: false,
            last: Mutex::new(Last {
                current: 0,
                total: 0,
                finished: false,
            }),
        }
    }

    /// 設定の欄だけで見せる仕事にする（上部の帯と「作業中」に数えない）。
    #[allow(dead_code)]
    pub fn background(mut self) -> Self {
        self.background = true;
        self
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }

    /// 途中の進捗。`total = 0` は終わりが分からない（不定）。終わりの後は何も送らない。
    pub fn update(&self, current: u32, total: u32, message: impl Into<String>) {
        let mut last = self.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.finished {
            return;
        }
        last.current = current;
        last.total = total;
        self.sink.send(JobProgress {
            job: self.job.clone(),
            current,
            total,
            message: message.into(),
            state: ProgressState::Running,
            background: self.background,
        });
    }

    /// 成功で終える。数は最後の `update` の total に揃える。
    pub fn done(&self, message: impl Into<String>) {
        self.finish(ProgressState::Done, message.into());
    }

    /// 件数を指定して成功で終える。
    pub fn done_with(&self, total: u32, message: impl Into<String>) {
        {
            let mut last = self.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if !last.finished {
                last.current = total;
                last.total = total;
            }
        }
        self.finish(ProgressState::Done, message.into());
    }

    pub fn fail(&self, message: impl Into<String>) {
        self.finish(ProgressState::Error, message.into());
    }

    pub fn cancel(&self, message: impl Into<String>) {
        self.finish(ProgressState::Cancelled, message.into());
    }

    fn finish(&self, state: ProgressState, message: String) {
        let mut last = self.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.finished {
            return;
        }
        last.finished = true;
        self.sink.send(final_progress(
            &self.job,
            self.background,
            last.current,
            last.total,
            Some((state, message)),
        ));
    }
}

impl<S: ProgressSink> Drop for ProgressGuard<S> {
    fn drop(&mut self) {
        let last = self.last.get_mut().unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.finished {
            return;
        }
        last.finished = true;
        let progress = final_progress(&self.job, self.background, last.current, last.total, None);
        self.sink.send(progress);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<JobProgress>>>);

    impl ProgressSink for Recorder {
        fn send(&self, progress: JobProgress) {
            self.0.lock().unwrap().push(progress);
        }
    }

    impl Recorder {
        fn all(&self) -> Vec<JobProgress> {
            self.0.lock().unwrap().clone()
        }
    }

    #[test]
    fn final_state_without_outcome_is_error() {
        let p = final_progress("scan", false, 3, 0, None);
        assert_eq!(p.state, ProgressState::Error);
        assert_eq!(p.message, INTERRUPTED_MESSAGE);
        assert_eq!((p.current, p.total), (3, 0));
    }

    #[test]
    fn final_done_fills_to_total_and_keeps_zero_items() {
        let p = final_progress("thumbs", false, 2, 5, Some((ProgressState::Done, "ok".into())));
        assert_eq!((p.current, p.total, p.state), (5, 5, ProgressState::Done));
        let zero = final_progress("thumbs", false, 0, 0, Some((ProgressState::Done, "0 件".into())));
        assert_eq!((zero.current, zero.total, zero.state), (0, 0, ProgressState::Done));
    }

    #[test]
    fn final_running_outcome_is_promoted_to_done() {
        let p = final_progress("x", true, 0, 0, Some((ProgressState::Running, "m".into())));
        assert_eq!(p.state, ProgressState::Done);
        assert!(p.background);
    }

    #[test]
    fn final_error_and_cancel_keep_last_numbers() {
        let p = final_progress("pull", false, 40, 100, Some((ProgressState::Error, "e".into())));
        assert_eq!((p.current, p.total, p.state), (40, 100, ProgressState::Error));
        let c = final_progress("pull", false, 40, 100, Some((ProgressState::Cancelled, "c".into())));
        assert_eq!(c.state, ProgressState::Cancelled);
    }

    #[test]
    fn guard_emits_error_on_drop_without_finish() {
        let rec = Recorder::default();
        {
            let guard = ProgressGuard::new(rec.clone(), "catalog");
            guard.update(0, 0, "取得しています");
        }
        let sent = rec.all();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].state, ProgressState::Running);
        assert_eq!(sent[1].state, ProgressState::Error);
        assert_eq!(sent[1].job, "catalog");
    }

    #[test]
    fn guard_sends_exactly_one_final_state() {
        let rec = Recorder::default();
        {
            let guard = ProgressGuard::new(rec.clone(), "thumbs");
            guard.done_with(0, "0 件のサムネイルを保存しました");
            guard.update(1, 2, "ignored after finish");
            guard.fail("ignored");
        }
        let sent = rec.all();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].state, ProgressState::Done);
        assert_eq!((sent[0].current, sent[0].total), (0, 0));
    }

    #[test]
    fn guard_background_flag_is_carried() {
        let rec = Recorder::default();
        {
            let guard = ProgressGuard::new(rec.clone(), "asset-updates").background();
            guard.update(1, 3, "1 / 3");
            guard.cancel("中止しました");
        }
        let sent = rec.all();
        assert!(sent.iter().all(|p| p.background));
        assert_eq!(sent[1].state, ProgressState::Cancelled);
        assert_eq!((sent[1].current, sent[1].total), (1, 3));
    }

    #[test]
    fn inferred_state_matches_legacy_rule() {
        assert_eq!(inferred_state(0, 0), ProgressState::Running);
        assert_eq!(inferred_state(1, 2), ProgressState::Running);
        assert_eq!(inferred_state(2, 2), ProgressState::Done);
    }

    #[test]
    fn payload_is_camel_case_and_old_payload_still_parses() {
        let value = serde_json::to_value(JobProgress {
            job: "scan".into(),
            current: 1,
            total: 2,
            message: "1 / 2".into(),
            state: ProgressState::Cancelled,
            background: true,
        })
        .unwrap();
        assert_eq!(value["state"], "cancelled");
        assert_eq!(value["background"], true);
        let old: JobProgress =
            serde_json::from_str(r#"{"job":"scan","current":0,"total":0,"message":"m"}"#).unwrap();
        assert_eq!(old.state, ProgressState::Running);
        assert!(!old.background);
    }
}
