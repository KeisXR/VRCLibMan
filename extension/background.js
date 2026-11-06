importScripts("download-route.js", "companion.js");

const LOCAL_BASE = "http://127.0.0.1:17321";
const companion = globalThis.VrcLibManCompanion;
const route = globalThis.VrcLibManRoute;
// Serialize storage mutations across events; persist everything needed after worker shutdown.
let serial = Promise.resolve();
function exclusive(run) {
  const next = serial.then(run, run);
  serial = next.catch(() => {});
  return next;
}
async function settings() {
  return chrome.storage.local.get({ pairingToken: '', routeDownloads: false, pendingDeliveries: [], downloadIntents: [] });
}
function normalized(token) { return String(token || '').replace(/[\s-]/g, '').toUpperCase(); }
async function localRequest(path, body, token) {
  const response = await fetch(`${LOCAL_BASE}${path}`, {
    method: 'POST', credentials: 'omit', signal: AbortSignal.timeout(8000),
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json', 'X-VRCLibMan-Extension-Version': chrome.runtime.getManifest().version },
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`アプリへの送信に失敗しました (${response.status})`);
  return response.json();
}
async function flush() {
  const stored = await settings();
  if (!stored.pairingToken) return;
  let failure = '';
  for (const entry of stored.pendingDeliveries) {
    if (entry.token !== normalized(stored.pairingToken)) continue;
    try {
      if (entry.product) await localRequest('/import', { format:'vrclibman.booth-export', version:1, products:[entry.product] }, stored.pairingToken);
      await localRequest('/download-finished', entry.notice, stored.pairingToken);
      const current = await settings();
      await chrome.storage.local.set({ pendingDeliveries:current.pendingDeliveries.filter(row => row.id !== entry.id) });
    } catch (error) {
      failure ||= error.message;
    }
  }
  await chrome.storage.local.set({ deliveryError: failure });
}
async function queueNotice(notice, intent, downloadId) {
  const stored = await settings();
  const profile = await chrome.storage.local.get({deliveryProfile:''});
  const namespace = profile.deliveryProfile || crypto.randomUUID();
  if (!profile.deliveryProfile) await chrome.storage.local.set({deliveryProfile:namespace});
  const id = `${namespace}-${downloadId}`;
  if (!stored.pendingDeliveries.some(row => row.id === id)) {
    await chrome.storage.local.set({ pendingDeliveries:[...stored.pendingDeliveries, {
      id, token:intent?.token || normalized(stored.pairingToken), product:intent?.product,
      notice:{ ...notice, variation:intent?.file.variation || '', ...(intent ? {downloadableId:intent.file.downloadableId} : {}), notificationId:id },
    }] });
  }
  // Remove an intent only once its completed notice is durable.
  if (intent) await chrome.storage.local.set({ downloadIntents:stored.downloadIntents.filter(row => row.id !== intent.id) });
}
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  let settled = false;
  const finish = decision => { if (!settled) { settled=true; decision ? suggest(decision) : suggest(); } };
  settings().then(stored => {
    const id = companion.downloadId(item.url);
    const intents = stored.downloadIntents.filter(row => row.file.downloadableId === id && (row.downloadId == null || row.downloadId === item.id));
    if (intents.length === 1) {
      const intent = intents[0];
      finish({ filename:`VRCLibMan/${intent.product.boothId}/${intent.file.filename}`, conflictAction:'uniquify' });
      exclusive(async () => {
        const current = await settings();
        await chrome.storage.local.set({downloadIntents:current.downloadIntents.map(row => row.id === intent.id ? {...row, downloadId:item.id} : row)});
      }).catch(() => {});
    } else finish(route.decideFilename(item, stored.routeDownloads));
  }).catch(() => finish(null));
  return true;
});
async function handleDownload(item) {
  if (item?.state === 'interrupted') {
    const stored = await settings();
    if (stored.downloadIntents.some(row => row.downloadId === item.id)) {
      await chrome.storage.local.set({
        downloadIntents: item.canResume ? stored.downloadIntents : stored.downloadIntents.filter(row => row.downloadId !== item.id),
        deliveryError: item.canResume ? '取得が中断しました。ブラウザのダウンロード画面で再開してください。' : '取得が中止されました。必要ならファイルを選び直してください。',
      });
    }
    return;
  }
  if (!item || item.state !== 'complete') return;
  const stored = await settings();
  const intent = stored.downloadIntents.find(row => row.downloadId === item.id);
  let notice = route.completionNotice(item);
  if (!notice || (!intent && (!stored.routeDownloads || !stored.pairingToken))) return;
  // Staged product ID must agree with the exact clicked file's product.
  if (intent && intent.product.boothId !== notice.boothId) return;
  if (/^(text\/html|application\/xhtml\+xml)(?:;|$)/i.test(item.mime || '')) {
    await chrome.storage.local.set({deliveryError:'取得した内容がログイン画面などのHTMLでした。BOOTHでログインを確認してください。', downloadIntents:stored.downloadIntents.filter(row => row.id !== intent?.id)});
    return;
  }
  if (!intent) {
    const session = await chrome.storage.session.get({pageNotes:[]});
    const note = session.pageNotes.find(row => row.boothId === notice.boothId);
    if (note) notice = {...notice, title:note.title || '', ...(note.isFree ? {isFree:true} : {}), ...(note.isAdult ? {isAdult:true} : {})};
  }
  await queueNotice({...notice, startedAt:item.startTime || intent?.startedAt || ''}, intent, item.id);
  await flush();
}
chrome.downloads.onChanged.addListener(delta => {
  if (['complete','interrupted'].includes(delta.state?.current)) exclusive(async () => {
    const [item] = await chrome.downloads.search({ id:delta.id });
    await handleDownload(item);
  }).catch(() => {});
});
async function reconcile() {
  const stored = await settings();
  for (const intent of stored.downloadIntents) {
    const items = intent.downloadId == null
      ? await chrome.downloads.search({ url:intent.url, startedAfter:intent.startedAt, limit:10 })
      : await chrome.downloads.search({ id:intent.downloadId });
    // Never infer a mapping when several downloads match a recovered intent.
    if (items.length === 1) {
      if (intent.downloadId == null) {
        const current = await settings();
        await chrome.storage.local.set({ downloadIntents:current.downloadIntents.map(row => row.id === intent.id ? {...row, downloadId:items[0].id} : row) });
      }
      await handleDownload(items[0]);
    }
  }
  await flush();
}
async function startDownload(message) {
  const tab = await chrome.tabs.get(message.tabId);
  if (!companion.boothUrl(tab.url)) throw new Error('BOOTHの商品ページかライブラリを開いてください');
  const response = await chrome.tabs.sendMessage(tab.id, {type:'extract-page'});
  const selected = companion.choose(response?.products, message.downloadableId);
  if (!selected) throw new Error('ファイルとバリエーションを一意に確認できません。ページを更新してください');
  const {product, file} = selected;
  if (!/^\d{4,12}$/.test(product.boothId) || route.safeDownloadName(file.filename) !== file.filename || !/\.(zip|unitypackage|rar|7z|vrm)$/i.test(file.filename)) throw new Error('このファイルは取得対象外です');
  const stored = await settings();
  if (!stored.pairingToken || !stored.routeDownloads) throw new Error('ペアリングと「ダウンロードを自動整理」を設定してください');
  if (stored.downloadIntents.some(row => row.file.downloadableId === file.downloadableId)) throw new Error('このファイルはすでに取得待ちです。ブラウザのダウンロード一覧を確認してください');
  const url = `https://booth.pm/downloadables/${file.downloadableId}`;
  const intent = {id:crypto.randomUUID(), token:normalized(stored.pairingToken), product, file, url, startedAt:new Date().toISOString()};
  await chrome.storage.local.set({downloadIntents:[...stored.downloadIntents,intent]});
  let id;
  try {
    // This handler is reachable only from the explicit file button in our popup.
    id = await chrome.downloads.download({ url, filename:`VRCLibMan/${product.boothId}/${file.filename}`, conflictAction:'uniquify' });
  } catch {
    await chrome.storage.local.set({downloadIntents:stored.downloadIntents});
    throw new Error('取得を開始できません。BOOTHへのログイン状態を確認してください');
  }
  const current = await settings();
  await chrome.storage.local.set({downloadIntents:current.downloadIntents.map(row => row.id === intent.id ? {...row, downloadId:id} : row)});
  return {ok:true};
}
chrome.runtime.onMessage.addListener((message, sender, respond) => {
  const isPopup = sender.id === chrome.runtime.id && sender.url === chrome.runtime.getURL('popup.html');
  const isBooth = sender.id === chrome.runtime.id && sender.tab && companion.boothUrl(sender.url);
  if (message?.type === 'product-status' && isBooth) {
    // Tokens and app responses never pass through page messages.
    exclusive(async () => {
      const stored = await settings();
      if (!stored.pairingToken) throw new Error('アプリとの接続を設定してください');
      return localRequest('/product-status', {ids:message.ids}, stored.pairingToken);
    }).then(respond, () => respond({error:'アプリに接続できません'}));
  } else if (message?.type === 'remember-download' && isBooth) {
    exclusive(async () => {
      const stored = await settings();
      if (!stored.routeDownloads || !stored.pairingToken) return {ok:true};
      const result = await chrome.tabs.sendMessage(sender.tab.id, {type:'extract-page'});
      const selected = companion.choose(result?.products,message.downloadableId);
      if (!selected || route.safeDownloadName(selected.file.filename) !== selected.file.filename || !/\.(zip|unitypackage|rar|7z|vrm)$/i.test(selected.file.filename)) return {ok:true};
      if (stored.downloadIntents.some(row => row.file.downloadableId === message.downloadableId)) return {ok:true};
      const intent={id:crypto.randomUUID(),token:normalized(stored.pairingToken),...selected,url:`https://booth.pm/downloadables/${message.downloadableId}`,startedAt:new Date().toISOString()};
      await chrome.storage.local.set({downloadIntents:[...stored.downloadIntents,intent]});
      return {ok:true};
    }).then(respond, () => respond({error:'ファイルの対応を記録できません'}));
  } else if (message?.type === 'start-download' && isPopup) {
    exclusive(() => startDownload(message)).then(respond, error => respond({error:error.message}));
  } else if (message?.type === 'retry-deliveries' && isPopup) {
    exclusive(reconcile).then(() => respond({ok:true}), () => respond({error:'再送できませんでした'}));
  } else return;
  return true;
});
chrome.alarms.onAlarm.addListener(alarm => { if (alarm.name === 'retry-deliveries') exclusive(reconcile).catch(() => {}); });
function initialize() {
  chrome.alarms.create('retry-deliveries', {periodInMinutes:1});
  exclusive(reconcile).catch(() => {});
}
chrome.runtime.onStartup.addListener(initialize);
chrome.runtime.onInstalled.addListener(initialize);
