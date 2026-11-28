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
        Task<string> pending;
        Action<string> onDone;
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

        // 取り込む商品を選んだあと。
        string importBoothId;
        string importTitle;
        FilesReply files;
        FileItem chosenFile;
        PackageItem chosenPackage;
        CheckReply check;
        string doneNote;

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
            if (pending != null) return;
            var json = JsonUtility.ToJson(body);
            busy = waiting;
            onDone = done;
            pending = Task.Run(() => Bridge.Post(route, json));
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

        PackageRequest ChosenRequest()
        {
            return new PackageRequest
            {
                project = ProjectFolder(),
                path = chosenFile.path,
                entries = string.IsNullOrEmpty(chosenPackage.entry) ? new string[0] : new[] { chosenPackage.entry },
                boothId = importBoothId,
            };
        }

        void Choose(FileItem file, PackageItem package)
        {
            chosenFile = file;
            chosenPackage = package;
            check = null;
            Run("check", ChosenRequest(), "このプロジェクトに足りないものを確かめています…", body =>
            {
                check = JsonUtility.FromJson<CheckReply>(body);
            });
        }

        void OpenImport()
        {
            var name = chosenPackage.name;
            Run("prepare", ChosenRequest(), "パッケージを用意しています…", body =>
            {
                var prepared = JsonUtility.FromJson<PrepareReply>(body);
                foreach (var package in prepared.packages ?? new PreparedItem[0])
                {
                    // Import 画面を出す。取り込むかどうかは本人が決める。
                    AssetDatabase.ImportPackage(package.path, true);
                }
                CloseImport();
                doneNote = "「" + name + "」の Import 画面を開きました。取り込んだあと「調べ直す」で確かめられます。";
                if (reply != null) reply.stale = true;
            });
        }

        void CloseImport()
        {
            importBoothId = null;
            importTitle = null;
            files = null;
            chosenFile = null;
            chosenPackage = null;
            check = null;
        }

        // ---- 描画 ----

        void OnGUI()
        {
            DrawHeader();
            if (!string.IsNullOrEmpty(busy)) EditorGUILayout.HelpBox(busy, MessageType.None);
            if (!string.IsNullOrEmpty(error)) EditorGUILayout.HelpBox(error, MessageType.Warning);
            if (!string.IsNullOrEmpty(doneNote)) EditorGUILayout.HelpBox(doneNote, MessageType.Info);
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
            if (chosenPackage == null)
            {
                EditorGUILayout.LabelField("取り込むパッケージを選んでください", EditorStyles.miniLabel);
                scroll = EditorGUILayout.BeginScrollView(scroll);
                foreach (var file in list)
                {
                    EditorGUILayout.LabelField(string.IsNullOrEmpty(file.variation) ? file.filename : file.filename + "（" + file.variation + "）", EditorStyles.boldLabel);
                    foreach (var package in file.packages ?? new PackageItem[0])
                    {
                        using (new EditorGUILayout.HorizontalScope())
                        {
                            EditorGUILayout.LabelField(package.name, EditorStyles.wordWrappedLabel);
                            using (new EditorGUI.DisabledScope(pending != null))
                            {
                                if (GUILayout.Button("選ぶ", GUILayout.Width(60))) Choose(file, package);
                            }
                        }
                    }
                }
                EditorGUILayout.EndScrollView();
                return;
            }
            EditorGUILayout.LabelField(chosenPackage.name, EditorStyles.wordWrappedLabel);
            if (check == null) return;
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
            foreach (var bundled in check.bundled ?? new BundledItem[0])
            {
                EditorGUILayout.HelpBox("このプロジェクトには " + bundled.name + " が入っています。同じものを重ねて取り込むとシェーダーが壊れることがあるので、取り込む必要はありません。", MessageType.Info);
            }
            using (new EditorGUILayout.HorizontalScope())
            {
                using (new EditorGUI.DisabledScope(pending != null))
                {
                    if (GUILayout.Button(check.installed ? "もう一度 Import 画面を開く" : "Import 画面を開く")) OpenImport();
                }
                if (GUILayout.Button("選び直す", GUILayout.Width(80)))
                {
                    chosenPackage = null;
                    chosenFile = null;
                    check = null;
                }
            }
        }
    }
}
