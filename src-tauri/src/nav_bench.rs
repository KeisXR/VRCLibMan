//! 画面の読み込みにかかる時間を、合成したライブラリで測る（docs/development.md#画面の読み込みの計測）。
//! ふだんの `cargo test` では走らない。
//!
//! ```sh
//! cargo test --release --manifest-path src-tauri/Cargo.toml --lib navigation_timing -- --ignored --nocapture
//! ```
//!
//! - `NAV_BENCH_SIZES`：商品数（既定 `300,1000,2000`）
//! - `NAV_BENCH_PLAIN`：アセット更新の対象なしの組も測る
//! - `NAV_BENCH_RUNS`：各計測の回数（既定 5、中央値を出す）
//! - `NAV_BENCH_DUMP`：コマンドの出力 JSON を書き出すフォルダ。一時フォルダのパスは `<tmp>` に置き換える
use crate::{asset_updates, db, library_files, purchased_editions, purchases, view};
use rusqlite::{Connection, params};
use std::time::{Duration, Instant};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

const WORDS: &[&str] = &[
    "対応アバター", "衣装", "テクスチャ", "ポリゴン数", "シェイプキー", "利用規約", "内容物", "更新履歴",
    "VRChatでの使用を想定した", "オリジナル3Dモデルです", "PhysBone設定済み", "Modular Avatar対応",
    "UnityPackage", "髪型", "アクセサリー", "マテリアル", "商用利用", "改変", "再配布", "禁止",
    "着せ替え", "素体", "同梱", "セット", "カラーバリエーション", "導入方法", "クレジット",
];

/// 800〜8000 字、平均 3000 字ほどの架空の説明。
fn description(rng: &mut Rng) -> String {
    let target = match rng.below(10) {
        0..=2 => 800 + rng.below(1200),
        3..=7 => 2000 + rng.below(2500),
        _ => 5000 + rng.below(3000),
    } as usize;
    let mut text = String::new();
    let mut chars = 0;
    while chars < target {
        let word = WORDS[rng.below(WORDS.len() as u64) as usize];
        text.push_str(word);
        chars += word.chars().count();
        text.push(if rng.chance(15) { '\n' } else { '、' });
        chars += 1;
    }
    text
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn time<T>(runs: usize, mut work: impl FnMut() -> T) -> (Duration, T) {
    let mut samples = Vec::new();
    let mut last = None;
    for _ in 0..runs.max(1) {
        let start = Instant::now();
        let value = work();
        samples.push(start.elapsed());
        last = Some(value);
    }
    (median(samples), last.unwrap())
}

fn ms(d: Duration) -> String {
    format!("{:8.1} ms", d.as_secs_f64() * 1000.0)
}

struct Built {
    connection: Connection,
    dir: tempfile::TempDir,
    popular_avatar: String,
}

/// 同梱の辞書の入った新しいライブラリに、架空の商品・対応・ファイル・購入履歴を入れる。
/// 乱数の種は商品数だけで決まるので、同じ商品数なら毎回同じ中身になる。
fn build(products: usize, watch: bool) -> Built {
    let dir = tempfile::tempdir().unwrap();
    let mut connection = db::open(&dir.path().join("library.sqlite")).unwrap();
    let mut rng = Rng(0x5eed + products as u64);
    let avatars: Vec<(String, String)> = {
        let mut q = connection.prepare("SELECT id, display_name FROM avatars ORDER BY id").unwrap();
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    };
    let files_root = dir.path().join("lib");
    std::fs::create_dir_all(&files_root).unwrap();
    let asset_genres: Vec<&str> = crate::genre::ASSET_GENRES.iter().map(|(k, _)| *k).collect();
    let world_genres: Vec<&str> = crate::genre::WORLD_GENRES.iter().map(|(k, _)| *k).collect();
    let owned = (products / 40).clamp(6, 60);
    let referenced = (products / 6).clamp(30, 260).min(avatars.len());
    let now = "2026-11-14T00:00:00Z";
    let mut watch_rows = Vec::new();
    let tx = connection.transaction().unwrap();
    for index in 0..products {
        let id = format!("{}", 4_000_000 + index);
        let (category, genre): (Option<&str>, Option<&str>) = if index < owned {
            (Some("avatar"), None)
        } else {
            match rng.below(100) {
                0..=44 => (Some("outfit"), Some("outfit")),
                45..=52 => (Some("hair"), Some("hair")),
                53..=62 => (Some("accessory"), Some("accessory")),
                63..=70 => (Some("texture"), Some(if rng.chance(50) { "makeup_skin" } else { "eye_texture" })),
                71..=80 => (Some("gimmick"), Some("gimmick")),
                81..=87 => (Some("world"), Some(world_genres[rng.below(world_genres.len() as u64) as usize])),
                88..=97 => (Some("other"), Some(asset_genres[12 + rng.below(2) as usize])),
                _ => (None, None),
            }
        };
        let body = description(&mut rng);
        let free = category != Some("avatar") && rng.chance(8);
        tx.execute(
            "INSERT INTO products (booth_id, title, shop, description_snippet, url, thumbnail_url, category, genre,
                is_generic, confidence, needs_review, imported_at, updated_at, extracted_at, classifier_version, is_free, is_adult)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0.7, 0, ?10, ?10, ?10, ?11, ?12, 0)",
            params![
                id,
                format!("【{}対応】サンプル商品 {index} / Sample Item", avatars[index % avatars.len()].1),
                format!("ショップ{}", index % 180),
                body,
                format!("https://booth.pm/ja/items/{id}"),
                format!("https://booth.pximg.net/c/300x300_a2_g5/{id}/i/{index}_base_resized.jpg"),
                category,
                genre,
                matches!(category, Some("gimmick" | "texture")) && rng.chance(40),
                now,
                crate::classifier::CLASSIFIER_VERSION,
                free,
            ],
        )
        .unwrap();
        // 対応アバター。前のほうのアバターに偏る。
        let mut linked: Vec<usize> = Vec::new();
        if index < owned {
            linked.push(index);
            tx.execute("UPDATE avatars SET owned = 1 WHERE id = ?1", [&avatars[index].0]).unwrap();
        } else if matches!(category, Some("outfit" | "hair" | "accessory" | "texture")) && rng.chance(80) {
            let count = match rng.below(10) {
                0..=4 => 1,
                5..=7 => 2 + rng.below(3),
                _ => 5 + rng.below(8),
            };
            for _ in 0..count {
                let skew = rng.below(referenced as u64).min(rng.below(referenced as u64)) as usize;
                if !linked.contains(&skew) {
                    linked.push(skew);
                }
            }
        }
        for avatar in &linked {
            tx.execute(
                "INSERT OR IGNORE INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, 'extracted')",
                params![id, avatars[*avatar].0],
            )
            .unwrap();
        }
        // バリエーション。複数対応の商品は対応アバターごとの版を持つ。
        let variation_names: Vec<String> = if linked.len() > 1 {
            let mut names: Vec<String> = linked.iter().map(|a| avatars[*a].1.clone()).collect();
            if rng.chance(30) {
                names.push("フルセット".into());
            }
            names
        } else if rng.chance(40) {
            vec!["Black".into(), "White".into()]
        } else {
            Vec::new()
        };
        for (position, name) in variation_names.iter().enumerate() {
            tx.execute(
                "INSERT OR IGNORE INTO product_variations (booth_id, name, owned) VALUES (?1, ?2, ?3)",
                params![id, name, position == 0],
            )
            .unwrap();
        }
        // ダウンロードファイル。半分ほどの商品は 1 件が手元にある。
        let file_count = 1 + rng.below(4) as usize;
        let has_local = !free && rng.chance(55);
        let mut file_keys = Vec::new();
        for file in 0..file_count {
            let filename = format!("item{index}_v1.{file}.zip");
            let downloadable = format!("{}", 7_000_000 + index * 8 + file);
            let local = (has_local && file == 0).then(|| {
                let folder = files_root.join(&id);
                std::fs::create_dir_all(&folder).unwrap();
                let path = folder.join(&filename);
                std::fs::write(&path, b"x").unwrap();
                path.display().to_string()
            });
            tx.execute(
                "INSERT INTO product_files (booth_id, variation, filename, size_bytes, downloaded, local_path, downloadable_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, variation_names.first().cloned().unwrap_or_default(), filename, 12_345_678, local.is_some(), local, downloadable],
            )
            .unwrap();
            if let Some(path) = &local {
                tx.execute(
                    "INSERT INTO local_files (path, filename, is_dir, size, linked_product_id, match_score, link_source, scanned_at)
                     VALUES (?1, ?2, 0, 1, ?3, 1.0, 'auto', ?4)",
                    params![path, filename, id, now],
                )
                .unwrap();
            }
            file_keys.push(downloadable);
        }
        if index % 2 == 0 {
            let order = format!("{}", 50_000_000 + index);
            tx.execute(
                "INSERT INTO purchase_orders (order_id, ordered_at, total_yen, source, recorded_at, updated_at, detail_read_at)
                 VALUES (?1, ?2, 1500, 'history', ?2, ?2, ?2)",
                params![order, format!("202{}-0{}-1{} 12:00", 3 + index % 4, 1 + index % 9, index % 9)],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO purchase_items (order_id, position, booth_id, name, shop, price_yen, quantity) VALUES (?1, 0, ?2, ?3, 'ショップ', 1500, 1)",
                params![order, id, format!("サンプル商品 {index}")],
            )
            .unwrap();
        }
        if watch {
            let snapshot = serde_json::json!({ "description": body, "files": [] });
            watch_rows.push(serde_json::json!({
                "boothId": id, "enabled": true, "generation": 0, "attemptedAt": 1_794_000_000i64,
                "checkedAt": now, "error": null, "baseline": snapshot, "latest": snapshot,
                "newFiles": [], "currentFiles": file_keys,
            }));
        }
    }
    tx.commit().unwrap();
    if watch {
        let blob = serde_json::json!({ "automatic": true, "allProducts": true, "generation": 1, "rows": watch_rows, "history": [] });
        db::set_setting(&connection, "asset_update_watch_v1", &blob.to_string()).unwrap();
    }
    db::set_setting(&connection, "library_folder", &files_root.display().to_string()).unwrap();
    Built { connection, popular_avatar: avatars[0].0.clone(), dir }
}

fn report(products: usize, watch: bool, runs: usize) {
    let Built { mut connection, dir, popular_avatar } = build(products, watch);
    let c = &mut connection;
    let count = |table: &str| -> i64 { c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap() };
    let blob: i64 = c
        .query_row("SELECT COALESCE(MAX(LENGTH(CAST(value AS BLOB))), 0) FROM settings WHERE key = 'asset_update_watch_v1'", [], |r| r.get(0))
        .unwrap();
    println!(
        "\n=== 商品 {products} 件 / アセット更新の対象 {} / アバター {} (所持 {}) / 対応リンク {} / ファイル行 {} / 手元 {} / 更新確認の保存値 {:.1} MB",
        if watch { "全商品" } else { "なし" },
        count("avatars"),
        count("avatars WHERE owned = 1"),
        count("product_avatars"),
        count("product_files"),
        count("local_files"),
        blob as f64 / 1_048_576.0,
    );

    // 読み込みの内訳
    let (t_snapshot, snapshot) = time(runs, || db::load_snapshot(c).unwrap());
    let (t_catalog, catalog) = time(runs, || purchased_editions::Catalog::load(c).unwrap());
    let (t_scopes, _) = time(runs, || catalog.scopes());
    let (t_watches, _) = time(runs, || asset_updates::file_watches(c).unwrap());
    println!("  load_snapshot 合計              {}", ms(t_snapshot));
    println!("    購入した対応版の読み込み      {}", ms(t_catalog));
    println!("    購入した対応版の判定          {}", ms(t_scopes));
    println!("    更新確認の保存値の解析        {}", ms(t_watches));

    // コマンド（commands.rs と同じ手順）
    let (t_stats, stats_json) = time(runs, || serde_json::to_string(&db::library_stats(c).unwrap()).unwrap());
    assert_eq!(stats_json, serde_json::to_string(&view::library_stats(&snapshot)).unwrap(), "counts read with SQL");
    let (t_cards_only, _) = time(runs, || view::avatar_cards(&snapshot));
    let (t_avatars, cards_json) = time(runs, || {
        let snapshot = db::load_snapshot(c).unwrap();
        serde_json::to_string(&view::avatar_cards(&snapshot)).unwrap()
    });
    let genre = |c: &Connection, kind: &str| {
        let genres = crate::genre::kind_genres(kind).unwrap();
        let all_ids = db::product_ids_by_genre(c, kind, None).unwrap();
        let mut snapshot = db::load_snapshot(c).unwrap();
        let remote = db::settings(c).unwrap().remote_library_folder;
        library_files::fill_local_places(&mut snapshot, &remote);
        serde_json::to_string(&view::genre_products(&snapshot, genres, &all_ids, None)).unwrap()
    };
    let (t_assets, assets_json) = time(runs, || genre(c, "asset"));
    let (t_worlds, worlds_json) = time(runs, || genre(c, "world"));
    let (t_places, _) = time(runs, || {
        let mut copy = snapshot.clone();
        library_files::fill_local_places(&mut copy, "");
    });
    let mut placed = snapshot.clone();
    library_files::fill_local_places(&mut placed, "");
    let asset_ids = db::product_ids_by_genre(c, "asset", None).unwrap();
    let (t_genre_view, _) = time(runs, || view::genre_products(&placed, crate::genre::ASSET_GENRES, &asset_ids, None));
    let (t_detail, detail_json) = time(runs, || {
        let mut snapshot = db::load_snapshot(c).unwrap();
        library_files::fill_local_places(&mut snapshot, "");
        serde_json::to_string(&view::avatar_detail(&snapshot, &popular_avatar).unwrap()).unwrap()
    });
    let (t_update_status, status_json) = time(runs, || serde_json::to_string(&asset_updates::status(c).unwrap()).unwrap());
    let (t_settings, _) = time(runs, || db::settings(c).unwrap());
    let (t_class, _) = time(runs, || db::classification_status(c).unwrap());
    let (t_knowledge, knowledge_json) = time(runs, || serde_json::to_string(&db::knowledge(c).unwrap()).unwrap());
    let (t_options, options_json) = time(runs, || serde_json::to_string(&db::product_options(c).unwrap()).unwrap());
    assert_eq!(options_json, serde_json::to_string(&view::product_options(&snapshot)).unwrap(), "options read with SQL");
    let (t_purchase_page, _) = time(runs, || serde_json::to_string(&purchases::page(c, &purchases::PurchaseQuery::default(), false).unwrap()).unwrap());
    let (t_purchase_status, _) = time(runs, || serde_json::to_string(&purchases::status(c, false).unwrap()).unwrap());
    let (t_purchase_summary, _) = time(runs, || serde_json::to_string(&purchases::summary(c, None).unwrap()).unwrap());

    let size = |json: &str| format!("{:.0} KB", json.len() as f64 / 1024.0);
    println!("  library_stats                   {}", ms(t_stats));
    println!("  list_avatars                    {}   （カードの計算 {}、{}）", ms(t_avatars), ms(t_cards_only).trim(), size(&cards_json));
    println!("  list_products_by_genre asset    {}   （手元の確認 {}、カードの組み立て {}、{}）", ms(t_assets), ms(t_places).trim(), ms(t_genre_view).trim(), size(&assets_json));
    println!("  list_products_by_genre world    {}   （{}）", ms(t_worlds), size(&worlds_json));
    println!("  get_avatar（対応の多い 1 体）   {}   （{}）", ms(t_detail), size(&detail_json));
    println!("  list_product_options            {}", ms(t_options));
    println!("  asset_update_status             {}   （{}）", ms(t_update_status), size(&status_json));
    println!("  get_settings                    {}", ms(t_settings));
    println!("  classification_status           {}", ms(t_class));
    println!("  list_knowledge                  {}", ms(t_knowledge));
    println!("  購入履歴 一覧 / 状態 / 集計     {} / {} / {}", ms(t_purchase_page).trim(), ms(t_purchase_status).trim(), ms(t_purchase_summary).trim());
    println!("  --- サイドバーから開いたときに画面が呼ぶ読み込みの合計 ---");
    println!("  アバター     {}", ms(t_avatars + t_stats));
    println!("  アセット     {}", ms(t_assets));
    println!("  ワールド     {}", ms(t_worlds));
    println!("  検索         {}", ms(t_avatars + t_assets + t_worlds));
    println!("  購入履歴     {}", ms(t_purchase_page + t_purchase_status));
    println!("  インポート   {}", ms(t_class + t_stats + t_settings + t_options));
    println!("  設定         {}", ms(t_settings + t_knowledge + t_update_status + t_options));

    if let Ok(out) = std::env::var("NAV_BENCH_DUMP") {
        let out = std::path::Path::new(&out).join(format!("{products}-{}", if watch { "watch" } else { "plain" }));
        std::fs::create_dir_all(&out).unwrap();
        // The temporary folder differs on every run; outputs are compared without it.
        let root = serde_json::to_string(&dir.path().display().to_string()).unwrap();
        let root = &root[1..root.len() - 1];
        let ms_of = |d: Duration| d.as_secs_f64() * 1000.0;
        let timings = serde_json::json!({
            "library_stats": ms_of(t_stats),
            "list_avatars": ms_of(t_avatars),
            "list_products_by_genre:asset": ms_of(t_assets),
            "list_products_by_genre:world": ms_of(t_worlds),
            "get_avatar": ms_of(t_detail),
            "asset_update_status": ms_of(t_update_status),
            "get_settings": ms_of(t_settings),
            "classification_status": ms_of(t_class),
            "list_knowledge": ms_of(t_knowledge),
            "popularAvatar": popular_avatar,
        })
        .to_string();
        for (name, body) in [
            ("library_stats.json", &stats_json),
            ("list_avatars.json", &cards_json),
            ("list_products_by_genre-asset.json", &assets_json),
            ("list_products_by_genre-world.json", &worlds_json),
            ("get_avatar.json", &detail_json),
            ("asset_update_status.json", &status_json),
            ("list_knowledge.json", &knowledge_json),
            ("timings.json", &timings),
        ] {
            std::fs::write(out.join(name), body.replace(root, "<tmp>")).unwrap();
        }
    }
}

#[test]
#[ignore = "計測。--release で --ignored を付けて実行する"]
fn navigation_timing() {
    let sizes: Vec<usize> = std::env::var("NAV_BENCH_SIZES")
        .ok()
        .map(|raw| raw.split(',').filter_map(|part| part.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![300, 1000, 2000]);
    let runs = std::env::var("NAV_BENCH_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let plain = std::env::var("NAV_BENCH_PLAIN").is_ok();
    for products in sizes {
        if plain {
            report(products, false, runs);
        }
        report(products, true, runs);
    }
}
