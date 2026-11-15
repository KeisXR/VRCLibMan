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

test('BOOTH page marks, unknown-preserving filter, explicit file buttons, disconnect recovery and the popup file button', {timeout:60000}, async()=>{
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
      ${source('popup.js')}
    })()`);
    await wait("[...document.querySelectorAll('#main button')].some(b=>b.textContent==='取得')");
    assert.match(await evaluate("document.querySelector('#main').textContent"),/見本A用/);
    await press("[...document.querySelectorAll('#main button')].find(b=>b.textContent==='取得')");
    await wait("window.started.some(message=>message.type==='start-download')");
    assert.deepEqual(await evaluate("window.started.filter(message=>message.type==='start-download')"),[{type:'start-download',tabId:1,downloadableId:'555555'}]);
    await wait("document.querySelector('#main').textContent.includes('取得を始めました')");
  }finally{
    socket?.close();chrome.kill();await vite.close();await pause(200);await rm(profile,{recursive:true,force:true,maxRetries:3,retryDelay:100});
  }
});
