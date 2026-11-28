// Unity の中の VRCLibMan の窓（メニューの Tools > VRCLibMan）。いま開いているプロジェクトに入っている商品と、
// 結び付けたアバターに対応していてまだ入っていない商品を並べ、押した商品のパッケージを Import 画面に渡す。
// 取り込むかどうかは Import 画面で本人が決める。プロジェクトへ直接は書き込まない。
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using UnityEditor;
using UnityEngine;

namespace VRCLibMan.Editor
{
    internal sealed class LibraryWindow : EditorWindow
    {
        ProjectReply reply;
        string error;
        string busy;
        Task<string[]> pending;
        Action<string[]> onDone;
        bool scannedOnce;
        string search = "";
        Vector2 scroll;
        bool showInstalled = true;
        bool showMissing = true;

        // 「ライブラリを探す」。
        static readonly string[] Tabs = { "このプロジェクト", "ライブラリを探す" };
        static readonly string[] KindLabels = { "すべて", "アセット", "ワールド", "アバター" };
        static readonly string[] KindValues = { "", "asset", "world", "avatar" };
        int tab;
        string libraryQuery = "";
        int libraryKind;
        SearchReply found;
        string foundFor;
        Vector2 libraryScroll;
        // 打つたびに探す。最後に打ってから少し待ち、前の問い合わせが終わってから送る。
        const double SearchDelay = 0.25;
        bool searchDirty;
        double lastEdit;

        // サムネイル。1 枚ずつアプリに頼み、読めたものを覚える（ほかの問い合わせとは別の枠）。
        const float ThumbSize = 48f;
        readonly Dictionary<string, Texture2D> thumbs = new Dictionary<string, Texture2D>();
        readonly HashSet<string> thumbMissing = new HashSet<string>();
        readonly HashSet<string> thumbQueued = new HashSet<string>();
        readonly Queue<string> thumbQueue = new Queue<string>();
        Task<byte[]> thumbTask;
        string thumbFor;

        // 取り込む商品を選んだあと。パッケージは「ファイルのパス + 改行 + zip の中の名前」で覚える。
        // 別のファイルのものも一緒に選べる（確かめと取り出しは、ファイルごとに順に頼む）。
        string importBoothId;
        string importTitle;
        FilesReply files;
        readonly HashSet<string> selected = new HashSet<string>();
        // 確かめたあと。null なら選んでいるところ。
        CheckReply check;
        // 確かめた時点で選んでいたもの（並びは一覧の順）と、そのうち渡すもの。
        readonly List<string> reviewed = new List<string>();
        readonly HashSet<string> keep = new HashSet<string>();
        // 要らない同梱版（キー → 入っている VPM パッケージの名前）。
        readonly Dictionary<string, string> bundledOwners = new Dictionary<string, string>();
        string doneNote;
        // Import 画面を順に開いている知らせに、最後に出した件数。
        string queueShown = "";

        [MenuItem("Tools/VRCLibMan")]
        static void Open()
        {
            var window = GetWindow<LibraryWindow>();
            window.titleContent = new GUIContent("VRCLibMan");
            window.minSize = new Vector2(360, 240);
            window.Show();
        }

        static string ProjectFolder()
        {
            return Directory.GetParent(Application.dataPath)?.FullName ?? "";
        }

        void OnEnable()
        {
            Reload();
        }

        void OnDisable()
        {
            foreach (var texture in thumbs.Values)
            {
                if (texture != null) DestroyImmediate(texture);
            }
            thumbs.Clear();
        }

        void Update()
        {
            UpdateThumbs();
            // Import 画面を順に開いているあいだ、進んだら件数を出し直す。
            var queue = ImportQueue.Active(out var position, out var total, out _) ? position + "/" + total : "";
            if (queue != queueShown)
            {
                queueShown = queue;
                Repaint();
            }
            if (searchDirty && pending == null && EditorApplication.timeSinceStartup - lastEdit >= SearchDelay)
            {
                searchDirty = false;
                SearchLibrary();
            }
            if (pending == null || !pending.IsCompleted) return;
            var task = pending;
            var done = onDone;
            pending = null;
            onDone = null;
            busy = null;
            if (task.IsFaulted)
            {
                var inner = task.Exception?.GetBaseException();
                error = inner is BridgeException ? inner.Message : "VRCLibMan とやりとりできません: " + inner?.Message;
            }
            else
            {
                error = null;
                try
                {
                    done?.Invoke(task.Result);
                }
                catch (ArgumentException)
                {
                    error = "VRCLibMan の応答を読めません。アプリとエディタ拡張の版をそろえてください。";
                }
            }
            Repaint();
        }

        void Run(string route, object body, string waiting, Action<string> done)
        {
            var json = JsonUtility.ToJson(body);
            RunAll(route, new[] { json }, waiting, bodies => done(bodies[0]));
        }

        /// 同じ問い合わせを順に送る（受信口は同時に 1 件だけ受ける）。1 件でも失敗したら、全体を失敗にする。
        void RunAll(string route, string[] jsons, string waiting, Action<string[]> done)
        {
            if (pending != null) return;
            busy = waiting;
            onDone = done;
            pending = Task.Run(() => jsons.Select(json => Bridge.Post(route, json)).ToArray());
        }

        void Reload()
        {
            Run("project", new ProjectRequest { project = ProjectFolder() }, "読み込んでいます…", body =>
            {
                reply = JsonUtility.FromJson<ProjectReply>(body);
                // 調べていない・古いときは、開いたときに 1 回だけ調べる（アプリのプロジェクトの詳細と同じ）。
                if (!scannedOnce && reply != null && (reply.scannedAt == 0 || reply.stale))
                {
                    Scan();
                }
            });
        }

        void Scan()
        {
            scannedOnce = true;
            Run("scan", new ProjectRequest { project = ProjectFolder() }, "照らし合わせています。書き換えません…", body =>
            {
                reply = JsonUtility.FromJson<ProjectReply>(body);
            });
        }

        void StartImport(string boothId, string title)
        {
            CloseImport();
            importBoothId = boothId;
            importTitle = title;
            Run("files", new FilesRequest { boothId = boothId }, "ファイルを読んでいます…", body =>
            {
                files = JsonUtility.FromJson<FilesReply>(body);
                // 1 つしか無ければ、選んでおく。
                var all = AllKeys();
                if (all.Count == 1) selected.Add(all[0]);
            });
        }

        void SearchLibrary()
        {
            var query = libraryQuery.Trim();
            if (query.Length == 0)
            {
                found = null;
                Repaint();
                return;
            }
            var label = query;
            // 打つたびに呼ぶので、待っている文は出さない（ちらつくため）。
            Run("search", new SearchRequest { project = ProjectFolder(), query = query, kind = KindValues[libraryKind] }, null, body =>
            {
                found = JsonUtility.FromJson<SearchReply>(body);
                foundFor = label;
                libraryScroll = Vector2.zero;
                // 新しい結果の分から頼み直す。
                thumbQueue.Clear();
                thumbQueued.Clear();
            });
        }

        // ---- サムネイル ----

        Texture2D Thumb(string boothId)
        {
            if (string.IsNullOrEmpty(boothId)) return null;
            if (thumbs.TryGetValue(boothId, out var texture)) return texture;
            if (!thumbMissing.Contains(boothId) && thumbFor != boothId && thumbQueued.Add(boothId)) thumbQueue.Enqueue(boothId);
            return null;
        }

        void UpdateThumbs()
        {
            if (thumbTask != null && thumbTask.IsCompleted)
            {
                var id = thumbFor;
                var task = thumbTask;
                thumbTask = null;
                thumbFor = null;
                if (task.IsFaulted || task.Result == null || task.Result.Length == 0)
                {
                    // 無い・読めないものは、窓を開き直すまで頼まない。接続できないときの文は本文の問い合わせに任せる。
                    thumbMissing.Add(id);
                }
                else
                {
                    var texture = new Texture2D(2, 2, TextureFormat.RGBA32, false) { hideFlags = HideFlags.HideAndDontSave };
                    if (texture.LoadImage(task.Result)) thumbs[id] = texture;
                    else
                    {
                        DestroyImmediate(texture);
                        thumbMissing.Add(id);
                    }
                }
                Repaint();
            }
            if (thumbTask != null || thumbQueue.Count == 0) return;
            var next = thumbQueue.Dequeue();
            thumbQueued.Remove(next);
            if (thumbs.ContainsKey(next) || thumbMissing.Contains(next)) return;
            thumbFor = next;
            var json = JsonUtility.ToJson(new FilesRequest { boothId = next });
            thumbTask = Task.Run(() => Bridge.PostBytes("thumb", json));
        }

        void DrawThumb(string boothId)
        {
            var rect = GUILayoutUtility.GetRect(ThumbSize, ThumbSize, GUILayout.Width(ThumbSize), GUILayout.Height(ThumbSize));
            var texture = Thumb(boothId);
            if (texture != null) GUI.DrawTexture(rect, texture, ScaleMode.ScaleAndCrop);
            else EditorGUI.DrawRect(rect, new Color(0.5f, 0.5f, 0.5f, 0.15f));
        }

        static string Key(FileItem file, PackageItem package)
        {
            return Key(file.path, package.entry);
        }

        static string Key(string path, string entry)
        {
            return path + "\n" + (entry ?? "");
        }

        /// 一覧にあるすべてのパッケージのキー（一覧の順）。
        List<string> AllKeys()
        {
            var keys = new List<string>();
            foreach (var file in files?.files ?? new FileItem[0])
            {
                foreach (var package in file.packages ?? new PackageItem[0]) keys.Add(Key(file, package));
            }
            return keys;
        }

        /// 選んだものを含むファイル（一覧の順）。zip の中のものは名前を並べ、
        /// ファイルそのもの（名前が空）はファイルだけを送る。
        List<PackageFile> ChosenFiles(ICollection<string> keys)
        {
            var chosenFiles = new List<PackageFile>();
            foreach (var file in files?.files ?? new FileItem[0])
            {
                var chosen = (file.packages ?? new PackageItem[0]).Where(package => keys.Contains(Key(file, package))).ToArray();
                if (chosen.Length == 0) continue;
                chosenFiles.Add(new PackageFile
                {
                    path = file.path,
                    entries = chosen.Where(package => !string.IsNullOrEmpty(package.entry)).Select(package => package.entry).ToArray(),
                });
            }
            return chosenFiles;
        }

        /// 選んだものを確かめる。ファイルごとの結果を 1 つにまとめ、要らない同梱版は渡すものから外しておく
        /// （アプリの「Unityで開く」と同じ。本人が選べば渡せる）。
        void CheckSelected()
        {
            var chosen = AllKeys().Where(selected.Contains).ToList();
            if (chosen.Count == 0) return;
            // 確かめはファイルごとに頼む（受信口の確かめは 1 つのファイルを見る）。
            var chosenFiles = ChosenFiles(chosen);
            var requests = chosenFiles.Select(file => JsonUtility.ToJson(new PackageRequest
            {
                project = ProjectFolder(),
                path = file.path,
                entries = file.entries,
                boothId = importBoothId,
            })).ToArray();
            RunAll("check", requests, "このプロジェクトに足りないものを確かめています…", bodies =>
            {
                var merged = new CheckReply { missing = new NeedItem[0], bundled = new BundledItem[0] };
                var needs = new List<NeedItem>();
                bundledOwners.Clear();
                for (var i = 0; i < bodies.Length; i++)
                {
                    var one = JsonUtility.FromJson<CheckReply>(bodies[i]);
                    if (one == null) continue;
                    merged.installed |= one.installed;
                    foreach (var need in one.missing ?? new NeedItem[0])
                    {
                        if (!needs.Any(seen => seen.kind == need.kind && seen.name == need.name)) needs.Add(need);
                    }
                    foreach (var bundled in one.bundled ?? new BundledItem[0])
                    {
                        bundledOwners[Key(chosenFiles[i].path, bundled.entry)] = bundled.name;
                    }
                }
                merged.missing = needs.ToArray();
                reviewed.Clear();
                reviewed.AddRange(chosen);
                keep.Clear();
                foreach (var key in chosen.Where(key => !bundledOwners.ContainsKey(key))) keep.Add(key);
                check = merged;
            });
        }

        void OpenImport()
        {
            var chosen = reviewed.Where(keep.Contains).ToList();
            // 前の Import 画面を順に開いているあいだは取り出さない（同じ名前のパッケージを取り出すと、
            // 待っているファイルを置き換えるため）。
            if (chosen.Count == 0 || ImportQueue.Active(out _, out _, out _)) return;
            // 取り出しは 1 回で頼む（別の zip の同じ名前のパッケージが、前のものを上書きしないように）。
            var request = new PackageRequest
            {
                project = ProjectFolder(),
                path = "",
                entries = new string[0],
                files = ChosenFiles(chosen).ToArray(),
                boothId = importBoothId,
            };
            Run("prepare", request, "パッケージを用意しています…", body =>
            {
                var prepared = new List<PreparedItem>(JsonUtility.FromJson<PrepareReply>(body)?.packages ?? new PreparedItem[0]);
                // Import 画面を 1 件ずつ出す。取り込むかどうかは、それぞれ本人が決める。
                ImportQueue.Enqueue(prepared.ToArray());
                CloseImport();
                doneNote = prepared.Count == 1
                    ? "「" + prepared[0].name + "」の Import 画面を開きました。取り込んだあと「調べ直す」で確かめられます。"
                    : prepared.Count + " 件の Import 画面を順に開きます。1 件を取り込むかキャンセルすると、次が開きます。終わったら「調べ直す」で確かめられます。";
                if (reply != null) reply.stale = true;
            });
        }

        void CloseImport()
        {
            importBoothId = null;
            importTitle = null;
            files = null;
            selected.Clear();
            ClearReview();
        }

        void ClearReview()
        {
            check = null;
            reviewed.Clear();
            keep.Clear();
            bundledOwners.Clear();
        }

        // ---- 描画 ----

        void OnGUI()
        {
            DrawHeader();
            if (!string.IsNullOrEmpty(busy)) EditorGUILayout.HelpBox(busy, MessageType.None);
            if (!string.IsNullOrEmpty(error)) EditorGUILayout.HelpBox(error, MessageType.Warning);
            if (!string.IsNullOrEmpty(doneNote)) EditorGUILayout.HelpBox(doneNote, MessageType.Info);
            DrawQueue();
            if (importBoothId != null)
            {
                DrawImport();
                return;
            }
            tab = GUILayout.Toolbar(tab, Tabs);
            if (tab == 1)
            {
                DrawLibrary();
                return;
            }
            if (reply == null) return;
            search = EditorGUILayout.TextField("絞り込み", search);
            scroll = EditorGUILayout.BeginScrollView(scroll);
            DrawInstalled();
            DrawMissing();
            EditorGUILayout.EndScrollView();
        }

        void DrawHeader()
        {
            using (new EditorGUILayout.HorizontalScope())
            {
                var name = reply?.project?.name ?? Path.GetFileName(ProjectFolder());
                EditorGUILayout.LabelField(name, EditorStyles.boldLabel);
                using (new EditorGUI.DisabledScope(pending != null))
                {
                    if (GUILayout.Button("読み直す", GUILayout.Width(80))) Reload();
                    if (GUILayout.Button("調べ直す", GUILayout.Width(80))) Scan();
                }
            }
            if (reply == null) return;
            EditorGUILayout.LabelField(ScannedLabel(), EditorStyles.miniLabel);
            if (reply.stale) EditorGUILayout.HelpBox("調べたあとにパッケージを渡したか、ライブラリが変わりました。「調べ直す」を押してください。", MessageType.Info);
            else if (reply.scannedAt != 0 && !reply.complete) EditorGUILayout.HelpBox("途中までしか調べられませんでした。ここに無い商品も、入っていないとは限りません。", MessageType.Info);
        }

        string ScannedLabel()
        {
            if (reply.scannedAt == 0) return "まだ調べていません";
            var minutes = (long)((DateTimeOffset.UtcNow.ToUnixTimeSeconds() - reply.scannedAt) / 60);
            if (minutes < 1) return "いま調べました";
            if (minutes < 60) return minutes + " 分前に調べました";
            if (minutes < 60 * 24) return (minutes / 60) + " 時間前に調べました";
            return DateTimeOffset.FromUnixTimeSeconds(reply.scannedAt).ToLocalTime().ToString("yyyy/M/d") + "に調べました";
        }

        bool Matches(string title)
        {
            if (string.IsNullOrWhiteSpace(search)) return true;
            var text = (title ?? "").ToLowerInvariant();
            return search.ToLowerInvariant().Split(new[] { ' ', '　' }, StringSplitOptions.RemoveEmptyEntries).All(text.Contains);
        }

        void DrawInstalled()
        {
            if (reply.stale) return;
            var items = (reply.products ?? new InstalledProduct[0]).Where(item => Matches(item.title)).ToArray();
            showInstalled = EditorGUILayout.Foldout(showInstalled, "入っている商品（" + items.Length + "）", true);
            if (!showInstalled) return;
            if (items.Length == 0 && reply.scannedAt != 0) EditorGUILayout.LabelField("見つかりませんでした", EditorStyles.miniLabel);
            foreach (var item in items)
            {
                using (new EditorGUILayout.HorizontalScope(EditorStyles.helpBox))
                {
                    DrawThumb(item.boothId);
                    using (new EditorGUILayout.VerticalScope())
                    {
                        using (new EditorGUILayout.HorizontalScope())
                        {
                            EditorGUILayout.LabelField(item.title, EditorStyles.wordWrappedLabel);
                            using (new EditorGUI.DisabledScope(pending != null))
                            {
                                if (GUILayout.Button("取り込む…", GUILayout.Width(80))) StartImport(item.boothId, item.title);
                            }
                        }
                        EditorGUILayout.LabelField(item.verdict == "installed" ? "導入済み" : "一部が入っています", EditorStyles.miniLabel);
                        if (!string.IsNullOrEmpty(item.note)) EditorGUILayout.LabelField(item.note, EditorStyles.wordWrappedLabel);
                        if (item.adjusted > 0) EditorGUILayout.LabelField("マテリアル・プレハブなど " + item.adjusted + " 個は配布物から変わっています", EditorStyles.wordWrappedMiniLabel);
                    }
                }
            }
        }

        void DrawMissing()
        {
            if (!reply.missingKnown) return;
            var items = (reply.missing ?? new MissingProduct[0]).Where(item => Matches(item.title)).ToArray();
            showMissing = EditorGUILayout.Foldout(showMissing, "まだ入っていない対応アセット（" + items.Length + "）", true);
            if (!showMissing) return;
            if (items.Length == 0)
            {
                var avatars = reply.project?.avatars ?? new string[0];
                EditorGUILayout.LabelField(avatars.Length == 0
                    ? "VRCLibMan の「プロジェクト」でアバターを結び付けると、対応アセットが並びます"
                    : "ありません", EditorStyles.wordWrappedMiniLabel);
            }
            foreach (var item in items)
            {
                using (new EditorGUILayout.HorizontalScope(EditorStyles.helpBox))
                {
                    DrawThumb(item.boothId);
                    EditorGUILayout.LabelField(item.title, EditorStyles.wordWrappedLabel);
                    using (new EditorGUI.DisabledScope(pending != null || item.files == 0))
                    {
                        if (GUILayout.Button(item.files == 0 ? "ファイルなし" : "取り込む…", GUILayout.Width(80))) StartImport(item.boothId, item.title);
                    }
                }
            }
        }

        static string KindLabel(SearchItem item)
        {
            if (!string.IsNullOrEmpty(item.genre)) return item.genre;
            switch (item.category)
            {
                case "avatar": return "アバター";
                case "outfit": return "衣装";
                case "hair": return "髪";
                case "accessory": return "アクセサリー";
                case "texture": return "テクスチャ";
                case "gimmick": return "ギミック";
                case "world": return "ワールド";
                case "other": return "その他";
                default: return "未分類";
            }
        }

        void DrawLibrary()
        {
            using (new EditorGUILayout.HorizontalScope())
            {
                EditorGUI.BeginChangeCheck();
                libraryQuery = EditorGUILayout.TextField(libraryQuery);
                libraryKind = EditorGUILayout.Popup(libraryKind, KindLabels, GUILayout.Width(80));
                if (EditorGUI.EndChangeCheck())
                {
                    searchDirty = true;
                    lastEdit = EditorApplication.timeSinceStartup;
                }
            }
            EditorGUILayout.LabelField("打つと、商品名・ショップ・説明・ジャンルから探します（空白で区切ると、すべてを含むもの）", EditorStyles.wordWrappedMiniLabel);
            if (found == null) return;
            var items = found.items ?? new SearchItem[0];
            EditorGUILayout.LabelField(found.total > items.Length
                ? "「" + foundFor + "」" + found.total + " 件のうち、先頭の " + items.Length + " 件"
                : "「" + foundFor + "」" + items.Length + " 件", EditorStyles.miniLabel);
            libraryScroll = EditorGUILayout.BeginScrollView(libraryScroll);
            foreach (var item in items)
            {
                using (new EditorGUILayout.HorizontalScope(EditorStyles.helpBox))
                {
                    DrawThumb(item.boothId);
                    using (new EditorGUILayout.VerticalScope())
                    {
                        using (new EditorGUILayout.HorizontalScope())
                        {
                            EditorGUILayout.LabelField(item.title, EditorStyles.wordWrappedLabel);
                            // 手元のファイルの記録が無くても、リモートにあれば取り込めるので、押せるままにする
                            // （無ければ次の画面で「手元に Unity パッケージがありません」と出る）。
                            using (new EditorGUI.DisabledScope(pending != null))
                            {
                                if (GUILayout.Button("取り込む…", GUILayout.Width(80))) StartImport(item.boothId, item.title);
                            }
                        }
                        var facts = KindLabel(item);
                        if (!string.IsNullOrEmpty(item.shop)) facts += "・" + item.shop;
                        if (item.adult) facts += "・R-18";
                        EditorGUILayout.LabelField(facts, EditorStyles.wordWrappedMiniLabel);
                        var avatars = item.avatars ?? new string[0];
                        if (avatars.Length > 0) EditorGUILayout.LabelField("対応: " + string.Join("、", avatars), EditorStyles.wordWrappedMiniLabel);
                        else if (item.generic) EditorGUILayout.LabelField("汎用", EditorStyles.wordWrappedMiniLabel);
                        if (item.installed == "installed") EditorGUILayout.LabelField("このプロジェクトに導入済み", EditorStyles.miniBoldLabel);
                        else if (item.installed == "partial") EditorGUILayout.LabelField("このプロジェクトに一部が入っています", EditorStyles.miniBoldLabel);
                    }
                }
            }
            EditorGUILayout.EndScrollView();
        }

        void DrawQueue()
        {
            if (!ImportQueue.Active(out var position, out var total, out var current)) return;
            if (total <= 1) return;
            using (new EditorGUILayout.HorizontalScope(EditorStyles.helpBox))
            {
                EditorGUILayout.LabelField("Import 画面を順に開いています（" + position + " / " + total + "）: " + current, EditorStyles.wordWrappedLabel);
                // Import 画面が閉じたかを確かめられない版の Unity でだけ、本人が進める
                // （確かめられるときは、閉じて取り込みが終わったら自動で進む）。
                if (ImportQueue.NeedsManualAdvance && GUILayout.Button("次を開く", GUILayout.Width(80))) ImportQueue.OpenNext();
                if (GUILayout.Button("残りをやめる", GUILayout.Width(90))) ImportQueue.Clear();
            }
        }

        string PackageLabel(string key)
        {
            foreach (var file in files?.files ?? new FileItem[0])
            {
                foreach (var package in file.packages ?? new PackageItem[0])
                {
                    if (Key(file, package) == key) return package.name;
                }
            }
            return key;
        }

        void DrawImport()
        {
            using (new EditorGUILayout.HorizontalScope())
            {
                EditorGUILayout.LabelField(importTitle, EditorStyles.boldLabel);
                if (GUILayout.Button("戻る", GUILayout.Width(60))) CloseImport();
            }
            if (files == null) return;
            var list = files.files ?? new FileItem[0];
            if (list.Length == 0)
            {
                EditorGUILayout.HelpBox("手元に Unity パッケージがありません。VRCLibMan で「ダウンロード」を押してから、もう一度お試しください。", MessageType.Info);
                return;
            }
            if (check == null)
            {
                DrawChoices(list);
                return;
            }
            DrawReview();
        }

        void DrawChoices(FileItem[] list)
        {
            var all = AllKeys();
            EditorGUILayout.LabelField("取り込むパッケージを選んでください。いくつでも選べます。", EditorStyles.wordWrappedMiniLabel);
            scroll = EditorGUILayout.BeginScrollView(scroll);
            foreach (var file in list)
            {
                EditorGUILayout.LabelField(string.IsNullOrEmpty(file.variation) ? file.filename : file.filename + "（" + file.variation + "）", EditorStyles.boldLabel);
                foreach (var package in file.packages ?? new PackageItem[0])
                {
                    var key = Key(file, package);
                    if (EditorGUILayout.ToggleLeft(package.name, selected.Contains(key))) selected.Add(key);
                    else selected.Remove(key);
                }
            }
            EditorGUILayout.EndScrollView();
            var count = all.Count(selected.Contains);
            using (new EditorGUILayout.HorizontalScope())
            {
                using (new EditorGUI.DisabledScope(pending != null || count == 0))
                {
                    if (GUILayout.Button(count == 0 ? "選んだものを確かめる" : "選んだものを確かめる（" + count + " 件）")) CheckSelected();
                }
                if (all.Count > 1)
                {
                    var everything = count == all.Count;
                    if (GUILayout.Button(everything ? "すべて外す" : "すべて選ぶ", GUILayout.Width(90)))
                    {
                        if (everything) selected.Clear();
                        else foreach (var key in all) selected.Add(key);
                    }
                }
            }
        }

        void DrawReview()
        {
            if (check.installed) EditorGUILayout.HelpBox("この商品は、このプロジェクトにもう入っています。", MessageType.Info);
            var needs = check.missing ?? new NeedItem[0];
            var tools = needs.Where(need => need.kind != "product").Select(need => need.name).ToArray();
            if (tools.Length > 0)
            {
                EditorGUILayout.HelpBox("このプロジェクトに無いもの: " + string.Join("、", tools) + "。無いまま取り込むと、マテリアルがピンク色になったり、コンポーネントが「Missing」になったりします。ALCOM で追加してから取り込んでください。", MessageType.Warning);
            }
            foreach (var need in needs.Where(need => need.kind == "product"))
            {
                EditorGUILayout.HelpBox("「" + need.name + "」のファイルを使っています。このプロジェクトには入っていないので、先に取り込んでください。", MessageType.Warning);
            }
            scroll = EditorGUILayout.BeginScrollView(scroll);
            foreach (var key in reviewed)
            {
                if (EditorGUILayout.ToggleLeft(PackageLabel(key), keep.Contains(key))) keep.Add(key);
                else keep.Remove(key);
                if (bundledOwners.TryGetValue(key, out var owner))
                {
                    EditorGUILayout.LabelField("このプロジェクトには " + owner + " が入っています。同じものを重ねて取り込むとシェーダーが壊れることがあるので、取り込む必要はありません。", EditorStyles.wordWrappedMiniLabel);
                }
            }
            EditorGUILayout.EndScrollView();
            var count = reviewed.Count(keep.Contains);
            var queued = ImportQueue.Active(out _, out _, out _);
            if (queued) EditorGUILayout.LabelField("前に選んだものの Import 画面を開いています。終わってから開けます。", EditorStyles.wordWrappedMiniLabel);
            else if (count > 1) EditorGUILayout.LabelField("Import 画面を 1 件ずつ順に開きます。それぞれ中身を確かめてから取り込めます。", EditorStyles.wordWrappedMiniLabel);
            using (new EditorGUILayout.HorizontalScope())
            {
                using (new EditorGUI.DisabledScope(pending != null || count == 0 || queued))
                {
                    var label = check.installed ? "もう一度 Import 画面を開く" : "Import 画面を開く";
                    if (GUILayout.Button(count > 1 ? label + "（" + count + " 件）" : label)) OpenImport();
                }
                if (GUILayout.Button("選び直す", GUILayout.Width(80))) ClearReview();
            }
        }
    }
}
