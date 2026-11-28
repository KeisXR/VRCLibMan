// VRCLibMan のアプリ（127.0.0.1:17321 の受信口）へ問い合わせる。
// 判定・検査・取り出しはアプリが行い、ここは問い合わせて結果を受け取るだけ。
// 認証コードは、アプリが設定で「Unity の中から使う」をオンにしたときだけアプリデータの unity-token に置く。
// 下の受け取る形のフィールドは JsonUtility だけが書くので、「割り当てられない」という警告を出さない。
#pragma warning disable CS0649
using System;
using System.IO;
using System.Net;
using System.Text;
using UnityEngine;

namespace VRCLibMan.Editor
{
    internal sealed class BridgeException : Exception
    {
        public BridgeException(string message) : base(message) { }
    }

    internal static class Bridge
    {
        const string DefaultIdentifier = "app.vrclibman.desktop";
        const string BaseUrl = "http://127.0.0.1:17321/unity/";
        const string TokenFile = "unity-token";
        const int TimeoutMs = 180000;

        /// アプリの識別子。確かめのために VRCLIBMAN_IDENTIFIER で変えられる（英小文字・数字・. だけ）。
        static string Identifier()
        {
            var value = Environment.GetEnvironmentVariable("VRCLIBMAN_IDENTIFIER");
            if (string.IsNullOrEmpty(value)) return DefaultIdentifier;
            foreach (var c in value)
            {
                if (!(c >= 'a' && c <= 'z') && !(c >= '0' && c <= '9') && c != '.') return DefaultIdentifier;
            }
            return value;
        }

        /// アプリのデータフォルダ（Tauri の app_data_dir と同じ場所）。
        static string DataRoot()
        {
            switch (Application.platform)
            {
                case RuntimePlatform.WindowsEditor:
                    return Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
                case RuntimePlatform.OSXEditor:
                    return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Personal), "Library", "Application Support");
                default:
                    var xdg = Environment.GetEnvironmentVariable("XDG_DATA_HOME");
                    if (!string.IsNullOrEmpty(xdg) && Path.IsPathRooted(xdg)) return xdg;
                    return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Personal), ".local", "share");
            }
        }

        static string ReadToken()
        {
            var path = Path.Combine(DataRoot(), Identifier(), TokenFile);
            try
            {
                var token = File.ReadAllText(path).Trim();
                if (token.Length == 32) return token;
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            throw new BridgeException("VRCLibMan の設定で「Unity の中から使う」をオンにしてください。オンにしたあと、もう一度「読み直す」を押してください。");
        }

        /// POST して本文を返す。別のスレッドから呼ぶ（Unity の窓を止めない）。
        public static string Post(string route, string json)
        {
            return Encoding.UTF8.GetString(PostBytes(route, json));
        }

        /// POST して本文をバイト列で返す（サムネイルの PNG）。別のスレッドから呼ぶ。
        public static byte[] PostBytes(string route, string json)
        {
            var token = ReadToken();
            var request = (HttpWebRequest)WebRequest.Create(BaseUrl + route);
            request.Method = "POST";
            request.ContentType = "application/json; charset=utf-8";
            request.Headers[HttpRequestHeader.Authorization] = "Bearer " + token;
            request.Timeout = TimeoutMs;
            request.ReadWriteTimeout = TimeoutMs;
            // 受信口は自分の PC の中なので、OS のプロキシを通さない。
            request.Proxy = null;
            var bytes = Encoding.UTF8.GetBytes(json);
            request.ContentLength = bytes.Length;
            try
            {
                using (var stream = request.GetRequestStream()) stream.Write(bytes, 0, bytes.Length);
                using (var response = (HttpWebResponse)request.GetResponse()) return ReadBytes(response);
            }
            catch (WebException error) when (error.Response is HttpWebResponse response)
            {
                var body = Encoding.UTF8.GetString(ReadBytes(response));
                var message = ErrorMessage(body);
                throw new BridgeException(string.IsNullOrEmpty(message)
                    ? "VRCLibMan が応答しません（" + (int)response.StatusCode + "）"
                    : message);
            }
            catch (WebException)
            {
                throw new BridgeException("VRCLibMan に接続できません。アプリを起動してから「読み直す」を押してください。");
            }
        }

        static byte[] ReadBytes(HttpWebResponse response)
        {
            using (response)
            using (var stream = response.GetResponseStream())
            using (var memory = new MemoryStream())
            {
                stream.CopyTo(memory);
                return memory.ToArray();
            }
        }

        [Serializable]
        class ErrorReply
        {
            public string error;
        }

        static string ErrorMessage(string body)
        {
            try
            {
                return JsonUtility.FromJson<ErrorReply>(body)?.error ?? "";
            }
            catch (ArgumentException)
            {
                return "";
            }
        }
    }

    // ---- 受け取る形（アプリの unity_bridge.rs と同じ名前） ----

    [Serializable]
    internal class ProjectRequest
    {
        public string project;
    }

    [Serializable]
    internal class FilesRequest
    {
        public string boothId;
    }

    [Serializable]
    internal class PackageRequest
    {
        public string project;
        public string path;
        public string[] entries;
        public string boothId;
    }

    [Serializable]
    internal class SearchRequest
    {
        public string project;
        public string query;
        public string kind;
    }

    [Serializable]
    internal class SearchItem
    {
        public string boothId;
        public string title;
        public string shop;
        public string category;
        public string genre;
        public string[] avatars;
        public bool generic;
        public bool hasFiles;
        public bool adult;
        public string installed;
    }

    [Serializable]
    internal class SearchReply
    {
        public long total;
        public SearchItem[] items;
    }

    [Serializable]
    internal class ProjectInfo
    {
        public string name;
        public string version;
        public string kind;
        public string[] avatars;
    }

    [Serializable]
    internal class InstalledProduct
    {
        public string boothId;
        public string title;
        public string verdict;
        public string note;
        public int adjusted;
    }

    [Serializable]
    internal class MissingProduct
    {
        public string boothId;
        public string title;
        public int files;
    }

    [Serializable]
    internal class ProjectReply
    {
        public ProjectInfo project;
        public long scannedAt;
        public bool complete;
        public bool stale;
        public InstalledProduct[] products;
        public MissingProduct[] missing;
        public bool missingKnown;
    }

    [Serializable]
    internal class PackageItem
    {
        public string entry;
        public string name;
        public long size;
    }

    [Serializable]
    internal class FileItem
    {
        public string filename;
        public string variation;
        public string path;
        public PackageItem[] packages;
    }

    [Serializable]
    internal class FilesReply
    {
        public string title;
        public FileItem[] files;
    }

    [Serializable]
    internal class NeedItem
    {
        public string kind;
        public string name;
    }

    [Serializable]
    internal class BundledItem
    {
        public string entry;
        public string name;
    }

    [Serializable]
    internal class CheckReply
    {
        public bool installed;
        public NeedItem[] missing;
        public BundledItem[] bundled;
    }

    [Serializable]
    internal class PreparedItem
    {
        public string name;
        public string path;
    }

    [Serializable]
    internal class PrepareReply
    {
        public PreparedItem[] packages;
    }
}
