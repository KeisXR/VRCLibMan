//! Cleanup for the language-model classifiers that were removed (Ollama, Apple, and an
//! OpenAI-compatible API).
//!
//! Older versions kept the classifier choice in `settings` and could keep a cloud API key in
//! the OS credential store (Windows Credential Manager / macOS Keychain, service = app
//! identifier, account = [`KEY_ACCOUNT`]). Both are removed once at startup. The credential
//! store is touched only when the old settings show the cloud classifier was set up, so other
//! users never see a credential prompt.

use rusqlite::{Connection, params};

/// Settings rows only the removed classifiers read.
const RETIRED_SETTINGS: &[&str] = &[
    "ollama_base_url",
    "ollama_model",
    "llm_backend",
    "cloud_base_url",
    "cloud_model",
    "unload_ollama_after_classification",
];

/// Account name the cloud classifier used in the OS credential store.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
const KEY_ACCOUNT: &str = "cloud-classifier-api-key";

/// Deletes the retired settings rows. Returns whether the cloud classifier had been set up,
/// so the caller knows whether a saved API key may be left.
pub fn retire_settings(connection: &Connection) -> Result<bool, String> {
    let cloud_used: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM settings
               WHERE (key = 'llm_backend' AND value = 'cloud')
                  OR (key = 'cloud_model' AND TRIM(value) <> ''))",
            [],
            |row| row.get(0),
        )
        .map_err(|err| format!("設定を読めません: {err}"))?;
    for key in RETIRED_SETTINGS {
        connection
            .execute("DELETE FROM settings WHERE key = ?1", params![key])
            .map_err(|err| format!("設定を整理できません: {err}"))?;
    }
    Ok(cloud_used)
}

/// Removes the saved cloud API key, if any. Best effort: a failure is only logged, and the
/// settings that would trigger another attempt are already gone.
pub fn remove_api_key(service: &str) {
    #[cfg(any(windows, target_os = "macos"))]
    {
        match keyring::Entry::new(service, KEY_ACCOUNT).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => eprintln!(
                "VRCLibMan: 使わなくなったクラウド分類の APIキーを OS の資格情報から削除できませんでした"
            ),
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = service;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_rows_go_and_report_a_configured_cloud() {
        let connection = crate::db::open_memory().unwrap();
        for (key, value) in [
            ("ollama_model", "qwen3.5:9b"),
            ("llm_backend", "cloud"),
            ("cloud_model", "sample-model"),
        ] {
            crate::db::set_setting(&connection, key, value).unwrap();
        }
        assert!(retire_settings(&connection).unwrap());
        let left: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key IN ('ollama_model', 'llm_backend', 'cloud_model')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        assert!(!retire_settings(&connection).unwrap());
    }

    #[test]
    fn a_local_model_user_has_no_key_to_remove() {
        let connection = crate::db::open_memory().unwrap();
        crate::db::set_setting(&connection, "llm_backend", "ollama").unwrap();
        crate::db::set_setting(&connection, "cloud_model", "  ").unwrap();
        assert!(!retire_settings(&connection).unwrap());
    }
}
