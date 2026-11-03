function pageNotes() {
  try {
    const products = globalThis.VrcLibManExtract.extractPage(document, location.href);
    return (products || [])
      .filter((product) => product && product.boothId)
      .map((product) => ({
        boothId: String(product.boothId),
        isFree: product.isFree === true,
        isAdult: product.isAdult === true,
        title: String(product.title || "").slice(0, 300),
      }));
  } catch {
    return [];
  }
}

function rememberPage() {
  const notes = pageNotes();
  if (notes.length === 0 || !chrome.storage || !chrome.storage.session) return;
  chrome.storage.session.get({ pageNotes: [] }, (data) => {
    const map = new Map((data.pageNotes || []).filter((row) => row && row.boothId).map((row) => [row.boothId, row]));
    for (const note of notes) map.set(note.boothId, note);
    const merged = [...map.values()].slice(-40);
    chrome.storage.session.set({ pageNotes: merged });
  });
}

rememberPage();

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!message || message.type !== "extract-page") return undefined;
  try {
    const products = globalThis.VrcLibManExtract.extractPage(document, location.href);
    rememberPage();
    sendResponse({ products });
  } catch (error) {
    sendResponse({ error: String(error) });
  }
  return true;
});

// Keep the library cache in the extension. No ownership data is put in page messages.
(function companionView() {
  const api = globalThis.VrcLibManCompanion;
  if (!api || !api.boothUrl(location.href)) return;
  const badges = new Map();
  let states = new Map();
  let selected = '';
  let generation = 0;
  let timer;
  let busy = false;
  let again = false;
  const panel = document.createElement('div');
  const shadow = panel.attachShadow({mode:'closed'});
  shadow.innerHTML = `<style>:host{display:block;margin:12px 0;font:13px/1.5 system-ui;color:#173e37}section{padding:10px;background:#effaf7;border:1px solid #9dc9bc;border-radius:8px}select,button{font:inherit;margin:4px;padding:4px}p{margin:4px 0}</style><section><strong>VRCLibMan</strong><label> 所持アバター <select aria-label="対応アバターで絞り込み"><option value="">すべて表示</option></select></label><button type="button">状態を更新</button><p role="status">アプリに接続しています</p></section>`;
  const select = shadow.querySelector('select');
  const status = shadow.querySelector('[role=status]');
  select.addEventListener('change', () => {selected=select.value; paint();});
  shadow.querySelector('button').addEventListener('click', () => refresh());
  function cards() {
    const rows=[];
    for (const link of document.querySelectorAll('a[href*="/items/"]')) {
      const url=api.boothUrl(link.href);
      const id=url && /\/items\/(\d{4,12})(?:\/|$)/.exec(url.pathname)?.[1];
      if (!id) continue;
      const card=link.closest('.item-card, article, .bg-white');
      if (!card || rows.some(row=>row.card===card)) continue;
      const ids = new Set([...card.querySelectorAll('a[href*="/items/"]')].map(a => /\/items\/(\d{4,12})(?:\/|$)/.exec(api.boothUrl(a.href)?.pathname || '')?.[1]).filter(Boolean));
      if (ids.size===1) rows.push({id,card});
    }
    if (/\/items\/\d+/.test(location.pathname)) {
      const id=/\/items\/(\d{4,12})/.exec(location.pathname)?.[1];
      const heading=document.querySelector('h1, h2');
      if (id && heading) rows.push({id,card:heading});
    }
    return rows;
  }
  function paint() {
    let hidden=0;
    for (const [card, info] of badges) {
      if (!card.isConnected) {badges.delete(card); continue;}
      const row=states.get(info.id);
      const text=api.labels(row).join(' · ');
      const avatars=(row?.avatars || []).map(a=>a.name+(a.viaBaseBody?'（共通素体）':'')).join(' / ');
      info.label.textContent=text+(avatars?` · ${avatars}`:' · 対応情報未確認');
      const filtered=!!selected && row?.avatars?.length>0 && !api.matches(row,selected);
      // Hide only roots that this view has annotated; restore their prior hidden state.
      card.hidden=filtered || info.wasHidden;
      if(filtered) hidden++;
    }
    status.textContent=`${hidden ? `${hidden}件を絞り込みで非表示。` : ''}対応情報が未確認の商品は表示します。保存記録は現在のファイル存在を保証しません。`;
  }
  async function refresh() {
    if (busy) { again=true; return; }
    const rows=cards();
    if (!rows.length) return;
    if (!panel.isConnected) rows[0].card.parentElement?.insertBefore(panel,rows[0].card);
    busy=true;
    const gen=++generation;
    try {
      const ids=[...new Set(rows.map(r=>r.id))];
      const next = new Map();
      for(let i=0;i<ids.length;i+=100) {
        const result=await chrome.runtime.sendMessage({type:'product-status',ids:ids.slice(i,i+100)});
        if(result?.error || !Array.isArray(result?.products)) throw new Error();
        for(const row of result.products) next.set(row.boothId,row);
      }
      if(gen!==generation) return;
      states=next;
      const names=new Map();
      for(const row of states.values()) for(const a of row.avatars) names.set(a.id,a.name);
      select.replaceChildren(new Option('すべて表示',''));
      for(const [id,name] of names) select.add(new Option(name,id));
      if(names.has(selected)) select.value=selected; else selected='';
      for(const {id,card} of rows) {
        const old=badges.get(card);
        if(old?.id===id) continue;
        if(old){old.host.remove();card.hidden=old.wasHidden;badges.delete(card);}
        const host=document.createElement('span');
        const root=host.attachShadow({mode:'closed'});
        const label=document.createElement('span');
        label.style.cssText='display:block;font:12px/1.6 system-ui;background:#effaf7;color:#173e37;padding:4px 6px;border-radius:5px;margin:4px 0;overflow-wrap:anywhere';
        root.append(label);card.append(host);
        badges.set(card,{id,label,host,wasHidden:card.hidden});
      }
      paint();
    } catch {
      for(const [card,info] of badges) {card.hidden=info.wasHidden;info.host.remove();}
      badges.clear();states.clear();selected='';select.value='';
      status.textContent='アプリに接続できません。アプリを起動し、拡張のペアリングを確認してください。';
    } finally {busy=false;if(again){again=false;refresh();}}
  }
  let signature='';
  const observer=new MutationObserver(() => {
    clearTimeout(timer);
    timer=setTimeout(()=>{
      const rows=cards();
      const next=rows.map(r=>r.id).join(',');
      if(next!==signature || rows.some(r=>!badges.has(r.card))) {signature=next;refresh();}
    },400);
  });
  observer.observe(document.body,{subtree:true,childList:true});
  window.addEventListener('focus',()=>refresh());
  refresh();
})();


// Observe the user's original download click without changing BOOTH's navigation.
document.addEventListener('click', event => {
  if (!event.isTrusted || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
  const anchor=event.target.closest?.('a[href], [data-href]');
  const value=anchor?.getAttribute('href') || anchor?.getAttribute('data-href');
  const id=globalThis.VrcLibManCompanion?.downloadId(value ? new URL(value,location.href).href : '');
  if (!id) return;
  chrome.runtime.sendMessage({type:'remember-download',downloadableId:id}).catch(()=>{});
},true);
