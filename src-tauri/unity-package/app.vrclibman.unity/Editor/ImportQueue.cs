// 選んだパッケージの Import 画面を 1 件ずつ開く。
// Unity の Import 画面は 1 枚だけなので、続けて ImportPackage を呼ぶと最後のものしか残らない。
// 1 件を取り込むかキャンセルして画面が閉じたら、次を開く。取り込むかどうかは、それぞれ本人が決める。
// スクリプトを取り込むと Unity はスクリプトを読み直し、static の値が消えるので、順番は SessionState に置く
// （エディタを閉じると消える）。
using System;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace VRCLibMan.Editor
{
    [InitializeOnLoad]
    internal static class ImportQueue
    {
        const string StateKey = "VRCLibMan.ImportQueue";
        // Import 画面を開いてから、閉じたと見るまで待つ秒数（画面が出るまでのあいだに次を開かないため）。
        const double SettleSeconds = 1.0;
        const double PollSeconds = 0.5;

        [Serializable]
        class State
        {
            public string[] paths = new string[0];
            public string[] names = new string[0];
            // いま開いている（開く）もの。
            public int index;
            // index の Import 画面を開いたか。
            public bool opened;
        }

        /// Unity の Import 画面の型。見つからない版の Unity では、閉じたかを画面で確かめず、
        /// 取り込み・キャンセルの知らせ（と窓の「次を開く」）だけで進む。
        static readonly Type PackageImportWindow = typeof(EditorWindow).Assembly.GetType("UnityEditor.PackageImport");

        static double openedAt;
        static double lastPoll;

        static ImportQueue()
        {
            AssetDatabase.importPackageCompleted += Finished;
            AssetDatabase.importPackageCancelled += Finished;
            AssetDatabase.importPackageFailed += (name, _) => Finished(name);
            EditorApplication.update += Poll;
            // スクリプトの読み直しのあと。開く前に読み直しが入ったものは、ここで開く。
            EditorApplication.delayCall += () =>
            {
                var state = Load();
                if (state != null && !state.opened) OpenCurrent();
            };
        }

        /// 待っているものを足す。何も開いていなければ、最初の 1 件をすぐ開く。
        public static void Enqueue(PreparedItem[] items)
        {
            if (items == null || items.Length == 0) return;
            var state = Load() ?? new State();
            var paths = new string[state.paths.Length + items.Length];
            var names = new string[paths.Length];
            Array.Copy(state.paths, paths, state.paths.Length);
            Array.Copy(state.names, names, state.names.Length);
            for (var i = 0; i < items.Length; i++)
            {
                paths[state.paths.Length + i] = items[i].path;
                names[state.paths.Length + i] = string.IsNullOrEmpty(items[i].name) ? Path.GetFileName(items[i].path) : items[i].name;
            }
            state.paths = paths;
            state.names = names;
            Save(state);
            if (!state.opened) OpenCurrent();
        }

        /// 待っている件数と、いま開いているもの（無ければ null）。
        public static bool Active(out int position, out int total, out string current)
        {
            var state = Load();
            position = 0;
            total = 0;
            current = null;
            if (state == null) return false;
            position = state.index + 1;
            total = state.paths.Length;
            current = state.names[state.index];
            return true;
        }

        /// Import 画面が開いているか。確かめられない版の Unity では false。
        static bool ImportWindowOpen()
        {
            return PackageImportWindow != null && Resources.FindObjectsOfTypeAll(PackageImportWindow).Length > 0;
        }

        /// Import 画面が閉じたかを確かめられない版の Unity か。そのときだけ、窓に「次を開く」を出す。
        /// 確かめられるときに本人が進めると、閉じたあと取り込んでいる途中で次を開いてしまうため。
        public static bool NeedsManualAdvance => PackageImportWindow == null;

        /// 確かめられないときに、本人が押して次へ進む。取り込みやスクリプトの読み直しの途中では進めない。
        public static void OpenNext()
        {
            if (!NeedsManualAdvance || EditorApplication.isCompiling || EditorApplication.isUpdating) return;
            var state = Load();
            if (state == null) return;
            Advance(state);
        }

        /// 残りを開かない（いま開いている Import 画面はそのまま）。
        public static void Clear()
        {
            SessionState.EraseString(StateKey);
        }

        static void Finished(string packageName)
        {
            var state = Load();
            if (state == null || !state.opened) return;
            // ほかの取り込みの知らせでは進まない。
            if (!string.Equals(packageName, Path.GetFileNameWithoutExtension(state.paths[state.index]), StringComparison.Ordinal)) return;
            Advance(state);
        }

        static void Poll()
        {
            if (PackageImportWindow == null) return;
            var now = EditorApplication.timeSinceStartup;
            if (now - lastPoll < PollSeconds) return;
            lastPoll = now;
            var state = Load();
            if (state == null || !state.opened) return;
            // スクリプトの読み直しのあとは、開いた時刻が分からないので、読み直しの終わりから数える。
            if (openedAt == 0) openedAt = now;
            if (now - openedAt < SettleSeconds || EditorApplication.isCompiling || EditorApplication.isUpdating) return;
            if (!ImportWindowOpen()) Advance(state);
        }

        static void Advance(State state)
        {
            state.index++;
            state.opened = false;
            if (state.index >= state.paths.Length)
            {
                Clear();
                return;
            }
            Save(state);
            // 取り込みの知らせの中では開かず、そのあとで開く。
            EditorApplication.delayCall += OpenCurrent;
        }

        static void OpenCurrent()
        {
            var state = Load();
            if (state == null || state.opened) return;
            var path = state.paths[state.index];
            state.opened = true;
            Save(state);
            openedAt = EditorApplication.timeSinceStartup;
            if (!File.Exists(path))
            {
                // 取り出したファイルが消えていたら、とばして次へ。
                Advance(state);
                return;
            }
            // Import 画面を出す。取り込むかどうかは本人が決める。
            AssetDatabase.ImportPackage(path, true);
        }

        static State Load()
        {
            var json = SessionState.GetString(StateKey, "");
            if (string.IsNullOrEmpty(json)) return null;
            try
            {
                var state = JsonUtility.FromJson<State>(json);
                if (state?.paths == null || state.names == null || state.names.Length != state.paths.Length) return null;
                if (state.index < 0 || state.index >= state.paths.Length) return null;
                return state;
            }
            catch (ArgumentException)
            {
                return null;
            }
        }

        static void Save(State state)
        {
            SessionState.SetString(StateKey, JsonUtility.ToJson(state));
        }
    }
}
