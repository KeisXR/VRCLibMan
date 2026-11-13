//! 取り込みのあとに、未分類の商品を続けて分類する。
//!
//! 「ライブラリ全取得」が最後まで終わったときと、拡張から商品情報を受け取ったとき（`POST /import`）に頼む。
//! 分類の規則は「未分類を分類」と同じ `commands::run_extract` で、ここは「いつ走らせるか」だけを決める。
//! 分類がすでに走っているときは待ちの印だけを立て、その分類が終わったあとに続けて行う。
//! 利用者が分類を中断したときは、待ちも捨てる。

use crate::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

static PENDING: AtomicBool = AtomicBool::new(false);

/// ライブラリ全取得のあとに続けて分類してよいか。中止したときと、途中で止まったとき（ログイン切れ、
/// 年齢確認、BOOTH の応答待ちで公開情報を読み残したとき）は分類しない。公開情報（カテゴリとタグ）が
/// 揃う前に分類すると、BOOTH のカテゴリを使えずにタイトルと説明だけで決めてしまうため。
pub(crate) fn after_walk(cancelled: bool, notice: &str) -> bool {
    !cancelled && notice.is_empty()
}

/// 未分類の商品があれば分類を頼む。始めた（または待ちに入れた）ときは true。
pub(crate) fn request(app: &AppHandle) -> bool {
    let has_work = {
        let state = app.state::<AppState>();
        let Ok(connection) = state.db.lock() else {
            return false;
        };
        crate::db::has_unclassified_work(&connection).unwrap_or(false)
    };
    if !has_work {
        return false;
    }
    PENDING.store(true, Ordering::SeqCst);
    kick(app);
    true
}

/// 分類が終わったあとに呼ぶ。中断したときは待ちを捨て、そうでなければ待ちがあれば続けて分類する。
pub(crate) fn after_run(app: &AppHandle, cancelled: bool) {
    if cancelled {
        PENDING.store(false, Ordering::SeqCst);
        return;
    }
    kick(app);
}

fn kick(app: &AppHandle) {
    let running = app
        .state::<AppState>()
        .extract_running
        .load(Ordering::SeqCst);
    if running || !PENDING.load(Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { drain(app).await });
}

async fn drain(app: AppHandle) {
    while PENDING.swap(false, Ordering::SeqCst) {
        match crate::commands::run_extract(&app, "unclassified").await {
            Ok(report) => {
                let _ = app.emit("library-changed", ());
                if report.cancelled {
                    PENDING.store(false, Ordering::SeqCst);
                    return;
                }
            }
            // ほかの分類が先に始まった。その分類が終わるとき（after_run か、この関数の次の周）に拾われる。
            Err(message) if message == crate::commands::EXTRACT_ALREADY_RUNNING => {
                PENDING.store(true, Ordering::SeqCst);
                return;
            }
            Err(message) => {
                crate::browser::browser_log(
                    &app,
                    &format!("取り込みのあとの分類を実行できませんでした: {message}"),
                );
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_only_after_a_walk_that_finished() {
        assert!(after_walk(false, ""));
        assert!(!after_walk(true, ""), "the user stopped the walk");
        assert!(
            !after_walk(false, "ログインが必要です"),
            "the walk stopped part way"
        );
        assert!(!after_walk(true, "ログインが必要です"));
    }
}
