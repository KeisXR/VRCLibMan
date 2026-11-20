import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {mkdtemp,rm} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
const pause=ms=>new Promise(r=>setTimeout(r,ms));
const chromePath=process.env.CHROME_PATH || '/usr/bin/chromium';
const source=file=>readFileSync(`extension/${file}`,'utf8');

test('BOOTH page marks, unknown-preserving filter, explicit file buttons, disconnect recovery, the popup file button and the side panel library', {timeout:60000}, async()=>{
  const {createServer}=await import('vite');
  const vite=await createServer({server:{host:'127.0.0.1',port:0,hmr:false},logLevel:'error'});
  await vite.listen();
  const profile=await mkdtemp(path.join(os.tmpdir(),'vrclibman-companion-'));
  const chrome=spawn(chromePath,['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank'],{stdio:['ignore','ignore','pipe']});
  let socket;
  try{
    let log='';
    const wsUrl=await new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>reject(new Error('Chromium did not start')),10000);
      chrome.stderr.on('data',chunk=>{log+=chunk;const m=/DevTools listening on (ws:\/\/\S+)/.exec(log);if(m){clearTimeout(timer);resolve(m[1]);}});
      chrome.on('exit',()=>{clearTimeout(timer);reject(new Error('Chromium exited'));});
    });
    socket=new WebSocket(wsUrl);await new Promise(r=>socket.addEventListener('open',r,{once:true}));
    let sequence=0;const pending=new Map();
    const call=(method,params={},sessionId)=>new Promise((resolve,reject)=>{const id=++sequence;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params,sessionId}));});
    socket.addEventListener('message',event=>{const msg=JSON.parse(event.data);const p=pending.get(msg.id);if(p){pending.delete(msg.id);msg.error?p.reject(new Error(JSON.stringify(msg.error))):p.resolve(msg.result);}});
    const {targetId}=await call('Target.createTarget',{url:'about:blank'});
    const {sessionId}=await call('Target.attachToTarget',{targetId,flatten:true});
    const page=(m,p)=>call(m,p,sessionId);
    const evaluate=async expression=>{
      const result=await page('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});
      if(result.exceptionDetails)throw new Error(result.exceptionDetails.text+' '+result.exceptionDetails.exception?.description);
      return result.result.value;
    };
    async function wait(expression){for(let i=0;i<150;i++){if(await evaluate(expression))return;await pause(50);}throw new Error('Timed out: '+expression);}
    async function press(expression){
      const point=await evaluate(`(() => {const node=${expression};node.scrollIntoView({block:'center'});const r=node.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
      await page('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount:1});
      await page('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount:1});
    }
    const base=`http://127.0.0.1:${vite.httpServer.address().port}`;
    await page('Page.navigate',{url:base});await wait("document.readyState==='complete'");
    await evaluate(`(() => {
      document.body.innerHTML='<main style="display:flex"><aside id="side" style="width:160px">絞り込み</aside><ul id="grid"><li class="item-card" id="a"><a href="https://booth.pm/ja/items/123456">衣装A</a><a id="native-download" href="https://booth.pm/downloadables/555555">ダウンロード</a></li><li class="item-card" id="b"><a href="https://booth.pm/ja/items/234567">衣装B</a></li><li class="item-card" id="unknown"><a href="https://booth.pm/ja/items/345678">未登録の商品</a></li></ul></main>';
      document.querySelector('#native-download').addEventListener('click',event=>event.preventDefault());
      window.roots=[];const attach=Element.prototype.attachShadow;Element.prototype.attachShadow=function(options){const root=attach.call(this,options);window.roots.push(root);return root;};
      window.offline=false;window.messages=[];
      const statuses={products:[
        {boothId:'123456',registered:true,owned:false,localRecorded:false,filesKnown:true,savedFiles:[],avatars:[{id:'a',name:'見本A',viaBaseBody:false}]},
        {boothId:'234567',registered:true,owned:true,localRecorded:true,filesKnown:true,savedFiles:[],avatars:[{id:'b',name:'見本B',viaBaseBody:true}]},
        {boothId:'345678',registered:false,avatars:[]}]};
      window.chrome={runtime:{onMessage:{addListener(){}},async sendMessage(message){
        window.messages.push(message);
        if(message.type==='page-state')return {paired:true,routeDownloads:true,showBadges:true};
        if(message.type==='product-status')return window.offline?{error:'offline'}:statuses;
        return {ok:true};
      }}};
      ${source('extract.js')}
      ${source('companion.js')}
      const product={boothId:'123456',title:'衣装A',downloads:[{downloadableId:'555555',filename:'sample.zip',variation:'見本A用'}]};
      VrcLibManExtract.extractPage=()=>[product];
      const original=VrcLibManCompanion.boothUrl;VrcLibManCompanion.boothUrl=value=>value===location.href?new URL(location.href):original(value);
      ${source('content.js')}
    })()`);
    const shadowText="window.roots.map(root=>root.textContent).join(' ')";
    await wait(`${shadowText}.includes('登録のみ')`);
    const text=await evaluate(shadowText);
    assert.match(text,/所持.*保存記録あり.*見本B・共通素体/);
    assert.doesNotMatch(text,/未登録/,'unregistered products get no mark outside the library');
    assert.match(text,/所持アバターで絞り込む/);
    // Like the purchase library, the list sits beside a filter sidebar in one row. The bar goes inside the list,
    // before the first card, so the row keeps its two columns.
    assert.equal(await evaluate("document.querySelector('main').children.length"),2);
    assert.equal(await evaluate("window.roots.find(root=>root.querySelector('select')).host.parentElement.id"),'grid');
    assert.equal(await evaluate("window.roots.find(root=>root.querySelector('select')).host.nextElementSibling.id"),'a');
    assert.ok(await evaluate("window.roots.some(root=>[...root.querySelectorAll('button')].some(b=>b.textContent==='LibMan で取得'))"));
    // Filter keeps unknown products visible.
    await evaluate("(() => {const s=window.roots.map(r=>r.querySelector('select')).find(Boolean);s.value='a';s.dispatchEvent(new Event('change'));})()");
    assert.equal(await evaluate("document.querySelector('#b').hidden"),true);
    assert.equal(await evaluate("document.querySelector('#unknown').hidden"),false);
    assert.equal(await evaluate("document.querySelector('#a').hidden"),false);
    // Only trusted clicks on our button start a download.
    await evaluate("window.roots.flatMap(root=>[...root.querySelectorAll('button')]).find(b=>b.textContent==='LibMan で取得').click()");
    assert.equal(await evaluate("window.messages.filter(m=>m.type==='start-download').length"),0,'synthetic clicks do nothing');
    await press("window.roots.flatMap(root=>[...root.querySelectorAll('button')]).find(b=>b.textContent==='LibMan で取得')");
    await wait("window.messages.some(m=>m.type==='start-download')");
    assert.deepEqual(await evaluate("window.messages.filter(m=>m.type==='start-download')"),[{type:'start-download',downloadableId:'555555'}]);
    // The native BOOTH link is observed, never replaced.
    await evaluate("document.querySelector('#native-download').click()");
    assert.equal(await evaluate("window.messages.filter(m=>m.type==='import-download').length"),0);
    await press("document.querySelector('#native-download')");
    await wait("window.messages.some(m=>m.type==='import-download')");
    assert.equal(await evaluate("window.messages.filter(m=>m.type==='remember-download').length"),1);
    // A lost connection removes marks and restores hidden cards.
    await evaluate("window.offline=true;window.dispatchEvent(new Event('focus'))");
    await wait("!window.roots.some(root=>root.host.isConnected&&root.textContent.includes('登録のみ'))");
    assert.equal(await evaluate("document.querySelector('#b').hidden"),false);
    assert.equal(await evaluate("window.roots.some(root=>root.host.isConnected&&root.querySelector('select'))"),false);
    await evaluate("window.offline=false;window.dispatchEvent(new Event('focus'))");
    await wait(`${shadowText}.includes('登録のみ')`);

    // The real popup with an item page: one explicit file button.
    await page('Page.navigate',{url:base+'/extension/popup.html'});await wait("document.querySelector('#main') !== null");
    await evaluate(`(() => {
      window.started=[];
      const product={boothId:'123456',title:'見本衣装',downloads:[{downloadableId:'555555',filename:'sample.zip',variation:'見本A用'}]};
      window.chrome={
        runtime:{getURL:p=>p,openOptionsPage(){},async sendMessage(message){window.started.push(message);if(message.type==='connection')return {hasToken:true,status:200};if(message.type==='product-status')return {products:[{boothId:'123456',registered:true,owned:true,localRecorded:false,filesKnown:true,savedFiles:[],avatars:[]}]};return {ok:true};}},
        storage:{onChanged:{addListener(){}},local:{async get(defaults){return {...defaults,pairingToken:'ABCD2345',routeDownloads:true};},async set(){}},session:{async get(){return {job:null};}}},
        tabs:{async query(){return [{id:1,url:'https://booth.pm/ja/items/123456',windowId:1}];},async sendMessage(){return {products:[product]};},create(){}}
      };
      document.querySelector('#main').replaceChildren();
      ${source('shared.js')}
      ${source('companion.js')}
      ${source('ui.js')}
      ${source('pagecards.js')}
      ${source('popup.js')}
    })()`);
    await wait("[...document.querySelectorAll('#main button')].some(b=>b.textContent==='取得')");
    assert.match(await evaluate("document.querySelector('#main').textContent"),/見本A用/);
    await press("[...document.querySelectorAll('#main button')].find(b=>b.textContent==='取得')");
    await wait("window.started.some(message=>message.type==='start-download')");
    assert.deepEqual(await evaluate("window.started.filter(message=>message.type==='start-download')"),[{type:'start-download',tabId:1,downloadableId:'555555'}]);
    await wait("document.querySelector('#main').textContent.includes('取得を始めました')");

    // The real side panel: the open page's card, then the library by avatar and genre.
    await page('Page.navigate',{url:base+'/extension/sidepanel.html'});await wait("location.pathname.endsWith('sidepanel.html')&&document.readyState==='complete'");
    await evaluate(`(() => {
      // Drop what the page's own scripts built before they stopped at the missing chrome object.
      document.querySelector('#panel-library').replaceChildren();
      for(const node of document.querySelectorAll('.view, #open-settings')) node.replaceWith(node.cloneNode(false));
      document.querySelector('#view-library').textContent='ライブラリ';
      window.sent=[];window.opened=[];window.moved=[];window.saved=[];window.fetched=[];
      window.listeners={storage:[],activated:[],updated:[]};
      window.online=true;
      window.currentTab={id:1,url:'https://booth.pm/ja/items/123456',windowId:1,status:'complete',active:true};
      const pageAvatars={
        '123456':[{id:'plum',name:'プラム',viaBaseBody:true},{id:'manuka',name:'マヌカ',viaBaseBody:false}],
        '234567':[{id:'plum',name:'プラム',viaBaseBody:false}]};
      const items=(prefix,count,extra={})=>Array.from({length:count},(_,i)=>({boothId:String(200001+i),title:prefix+(i+1),shop:'見本の店',isAdult:false,unpublished:false,viaBaseBody:false,hasThumbnail:i===0,...extra}));
      const pageId=()=>/items\\/(\\d+)/.exec(window.currentTab.url||'')?.[1]||'';
      window.chrome={
        runtime:{getURL:p=>'chrome-extension://test/'+p,getManifest:()=>({version:'0.9.1'}),openOptionsPage(){},async sendMessage(message){
          window.sent.push(message);
          if(message.type==='connection')return {hasToken:true,status:window.online?200:null};
          if(message.type==='product-status')return {products:message.ids.map(id=>({boothId:id,registered:true,owned:true,localRecorded:false,filesKnown:true,savedFiles:[],avatars:pageAvatars[id]||[]}))};
          if(message.type==='library-avatars')return {avatars:[{id:'lime',displayName:'ライム',thumbnailId:'100001',itemCount:3},{id:'plum',displayName:'プラム',thumbnailId:null,itemCount:36},{id:'manuka',displayName:'マヌカ',thumbnailId:null,itemCount:35}],assetCount:40,worldCount:2};
          if(message.type==='library-items'){
            if(message.kind)return {sections:[{key:'outfit',label:'衣装',items:items('すべての衣装',40)}]};
            return {sections:[{key:'avatar',label:'アバター本体',items:items(message.avatar+'の本体',1)},{key:'outfit',label:'衣装',items:items(message.avatar+'の衣装',35,{viaBaseBody:message.avatar==='plum'})}]};
          }
          if(message.type==='library-product')return {boothId:message.id,historyRead:true,orderCount:2,orders:[{orderId:'12345678',orderedAt:'2026-03-14T21:30:00+09:00',gift:false}],edition:{mode:'specific',avatars:['マヌカ']}};
          return {ok:true};
        }},
        storage:{onChanged:{addListener(fn){window.listeners.storage.push(fn);}},
          local:{async get(defaults){return {...defaults,pairingToken:'ABCD-2345',routeDownloads:true};},async set(values){window.saved.push(values);}},
          session:{async get(defaults){return {...defaults};},async remove(){}}},
        tabs:{async query(){return [window.currentTab];},
          async sendMessage(){const id=pageId();return {products:id?[{boothId:id,title:'見本衣装'+id,downloads:[{downloadableId:'555555',filename:'sample.zip',variation:'見本A用'}]}]:[]};},
          create(options){window.opened.push(options);},update(id,options){window.moved.push([id,options]);},reload(){},
          onActivated:{addListener(fn){window.listeners.activated.push(fn);}},onUpdated:{addListener(fn){window.listeners.updated.push(fn);}}},
        downloads:{async search(){return [];}}
      };
      window.fetch=async(url,options)=>{window.fetched.push({url,body:options.body,auth:options.headers.Authorization,credentials:options.credentials});return {ok:true,async blob(){return new Blob(['x'],{type:'image/png'});}};};
      ${source('shared.js')}
      ${source('companion.js')}
      ${source('ui.js')}
      ${source('pagecards.js')}
      ${source('library.js')}
      ${source('sidepanel.js')}
    })()`);
    const cardsText="document.querySelector('#cards').textContent";
    const scopes="[...document.querySelectorAll('.scope')]";
    const pressed=`${scopes}.find(b=>b.getAttribute('aria-pressed')==='true')?.querySelector('.scope-name').textContent`;
    const rows="document.querySelectorAll('.genre[open] .prod').length";
    await wait(`${cardsText}.includes('購入した対応版')`);
    const card=await evaluate(cardsText);
    assert.match(card,/開いている商品見本衣装123456/);
    assert.match(card,/購入：2026\/3\/14・ほか 1 件/);
    assert.match(card,/購入した対応版：マヌカ/);
    assert.doesNotMatch(card,/円|¥/,'no price in the side panel');
    assert.match(card,/sample\.zip/);
    // A registered product can be opened in the app; the request carries the product ID only.
    await press("[...document.querySelectorAll('#cards button')].find(b=>b.textContent==='アプリで開く')");
    await wait("window.sent.some(m=>m.type==='open-in-app')");
    assert.deepEqual(await evaluate("window.sent.find(m=>m.type==='open-in-app')"),{type:'open-in-app',id:'123456'});
    await wait(`${cardsText}.includes('VRCLibMan でこの商品を開きました')`);
    // The page's avatars lead the list (direct before shared-body), and the first is selected.
    await wait(`${pressed}==='マヌカ'`);
    assert.deepEqual(await evaluate(`${scopes}.map(b=>b.querySelector('.scope-name').textContent+(b.querySelector('.scope-page')?'*':''))`),['マヌカ*','プラム*','ライム','すべて','ワールド']);
    await wait(`${rows}===31`);
    assert.deepEqual(await evaluate("[...document.querySelectorAll('.genre summary')].map(s=>s.textContent)"),['アバター本体1','衣装35']);
    assert.equal(await evaluate("document.querySelector('.more').textContent"),'さらに表示（残り 5 件）');
    assert.doesNotMatch(await evaluate("document.querySelector('.sections').textContent"),/null|undefined/);
    await press("document.querySelector('.more')");
    await wait(`${rows}===36`);
    // Thumbnails come from the app with the pairing code, one request per saved image.
    await wait("document.querySelector('.prod .thumb img')?.src.startsWith('blob:')");
    assert.ok(await evaluate("window.fetched.length>0&&window.fetched.every(row=>row.url==='http://127.0.0.1:17321/library/thumb'&&row.auth==='Bearer ABCD-2345'&&row.credentials==='omit')"));
    assert.equal(await evaluate("window.fetched.some(row=>row.body==='{\"id\":\"200001\"}')"),true);
    // A product opens in the BOOTH tab being viewed; Ctrl opens a background tab instead.
    await press("document.querySelector('.genre[open] .prod')");
    await wait("window.moved.length===1");
    assert.deepEqual(await evaluate("window.moved[0]"),[1,{url:'https://booth.pm/ja/items/200001'}]);
    const point=await evaluate("(() => {const node=document.querySelectorAll('.genre[open] .prod')[1];node.scrollIntoView({block:'center'});const r=node.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()");
    await page('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount:1,modifiers:2});
    await page('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount:1,modifiers:2});
    await wait("window.opened.length===1");
    assert.deepEqual(await evaluate("window.opened[0]"),{url:'https://booth.pm/ja/items/200001',active:false});
    assert.equal(await evaluate("window.moved.length"),1);
    // A manual choice is saved and survives a refresh of the same page.
    await press(`${scopes}.find(b=>b.textContent.includes('ライム'))`);
    await wait(`${pressed}==='ライム'`);
    assert.deepEqual(await evaluate("window.saved.at(-1)"),{panelScope:'avatar:lime'});
    await evaluate("window.dispatchEvent(new Event('focus'))");
    await pause(300);
    assert.equal(await evaluate(pressed),'ライム');
    // Another product: its avatar is selected, and items through the shared body are marked.
    await evaluate("window.currentTab={...window.currentTab,url:'https://booth.pm/ja/items/234567'};window.listeners.updated[0](1,{url:window.currentTab.url},window.currentTab)");
    await wait(`${pressed}==='プラム'`);
    await wait(`${rows}===31`);
    assert.match(await evaluate("document.querySelector('.genre[open]:nth-of-type(2) .prod').textContent"),/共通素体/);
    assert.equal(await evaluate("window.saved.at(-1).panelScope"),'avatar:lime','following a page is not saved as the user\'s choice');
    // The filter narrows by name and shop.
    await evaluate("(() => {const input=document.querySelector('.lib input');input.value='衣装 35';input.dispatchEvent(new Event('input'));})()");
    await wait(`${rows}===1`);
    await evaluate("(() => {const input=document.querySelector('.lib input');input.value='';input.dispatchEvent(new Event('input'));})()");
    // Opting out: the app's order, no page mark, and the selection stays when the page changes.
    await evaluate("window.listeners.storage[0]({followPageAvatar:{newValue:false}},'local')");
    assert.deepEqual(await evaluate(`${scopes}.map(b=>b.querySelector('.scope-name').textContent+(b.querySelector('.scope-page')?'*':''))`),['ライム','プラム','マヌカ','すべて','ワールド']);
    await evaluate("window.currentTab={...window.currentTab,url:'https://booth.pm/ja/items/123456'};window.listeners.updated[0](1,{url:window.currentTab.url},window.currentTab)");
    await wait(`${cardsText}.includes('見本衣装123456')`);
    await pause(300);
    assert.equal(await evaluate(pressed),'プラム');
    // 「すべて」 starts with closed headings; opening one lists the first page.
    await press(`${scopes}.find(b=>b.textContent.startsWith('すべて'))`);
    await wait("document.querySelector('.genre summary')?.textContent==='衣装40'");
    assert.equal(await evaluate(rows),0);
    await press("document.querySelector('.genre summary')");
    await wait(`${rows}===30`);
    // Away from BOOTH there is no page card, the library stays, and a product opens in a new tab.
    await evaluate("window.currentTab={id:2,windowId:1,status:'complete',active:true};window.listeners.activated[0]({tabId:2,windowId:1})");
    await wait(`${cardsText}===''`);
    await press("document.querySelector('.genre[open] .prod')");
    await wait("window.opened.length===2");
    assert.deepEqual(await evaluate("window.opened[1]"),{url:'https://booth.pm/ja/items/200001',active:true});
    // 「記録」 replaces the library.
    await press("document.querySelector('#view-log')");
    await wait("document.querySelector('#panel-library').hidden");
    assert.match(await evaluate("document.querySelector('#panel-log').textContent"),/まだ記録はありません/);
    await press("document.querySelector('#view-library')");
    await wait("!document.querySelector('#panel-library').hidden");
    // With the app stopped, nothing old stays on screen.
    await evaluate("window.online=false;window.dispatchEvent(new Event('focus'))");
    await wait("document.querySelector('.sections').textContent.includes('アプリを起動すると')");
    assert.equal(await evaluate("document.querySelectorAll('.prod').length"),0);
    assert.equal(await evaluate("document.querySelector('.scopes').hidden"),true);
  }finally{
    socket?.close();chrome.kill();await vite.close();await pause(200);await rm(profile,{recursive:true,force:true,maxRetries:3,retryDelay:100});
  }
});
