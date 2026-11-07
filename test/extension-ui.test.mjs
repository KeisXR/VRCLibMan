import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {mkdtemp,rm} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
const pause=ms=>new Promise(r=>setTimeout(r,ms));
const chromePath=process.env.CHROME_PATH || '/usr/bin/chromium';

test('browser companion badges, unknown-preserving avatar filter, disconnect recovery and popup click', {timeout:45000}, async()=>{
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
    const base=`http://127.0.0.1:${vite.httpServer.address().port}`;
    await page('Page.navigate',{url:base});await wait("document.readyState==='complete'");
    const companion=readFileSync('extension/companion.js','utf8');
    const extract=readFileSync('extension/extract.js','utf8');
    const content=readFileSync('extension/content.js','utf8');
    await evaluate(`(() => {
      document.body.innerHTML='<main><article id="a"><a href="https://booth.pm/ja/items/123456">衣装A</a></article><article id="b"><a href="https://booth.pm/ja/items/234567">衣装B</a></article><article id="unknown"><a href="https://booth.pm/ja/items/345678">未確認の商品</a></article></main>';
      window.roots=[];const attach=Element.prototype.attachShadow;Element.prototype.attachShadow=function(options){const root=attach.call(this,options);window.roots.push(root);return root;};
      window.offline=false;
      window.chrome={storage:{session:{get(defaults,callback){callback(defaults);},set(){}}},runtime:{onMessage:{addListener(){}},async sendMessage(){if(window.offline)return {error:'offline'};return {products:[{boothId:'123456',registered:true,owned:false,localRecorded:true,filesKnown:true,avatars:[{id:'a',name:'見本A',viaBaseBody:false}]},{boothId:'234567',registered:true,owned:true,localRecorded:false,filesKnown:true,avatars:[{id:'b',name:'見本B',viaBaseBody:true}]},{boothId:'345678',registered:false,avatars:[]}]};}}};
      ${extract}
      ${companion}
      const original=VrcLibManCompanion.boothUrl;VrcLibManCompanion.boothUrl=value=>value===location.href?new URL(location.href):original(value);
      ${content}
    })()`);
    await wait("window.roots.some(root=>root.textContent.includes('所持未確認'))");
    await evaluate(`(() => {
      window.importMessages=[];
      const previous=chrome.runtime.sendMessage;
      chrome.runtime.sendMessage=async message=>{window.importMessages.push(message);return message.type==='import-download'?{ok:true}:previous(message);};
      const anchor=document.createElement('a');anchor.id='native-download';anchor.href='https://booth.pm/downloadables/555555';anchor.textContent='見本ファイルをダウンロード';
      anchor.addEventListener('click',event=>event.preventDefault());document.querySelector('main').prepend(anchor);
    })()`);
    await wait("window.roots.some(root=>root.querySelector('a[href=\"vrclibman://import\"]'))");
    const importPoint=await evaluate("(() => {const link=window.roots.flatMap(root=>[...root.querySelectorAll('a')]).find(a=>a.textContent==='LibManでインポート');const r=link.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()");
    await page('Input.dispatchMouseEvent',{type:'mousePressed',...importPoint,button:'left',clickCount:1});
    await page('Input.dispatchMouseEvent',{type:'mouseReleased',...importPoint,button:'left',clickCount:1});
    await wait("window.importMessages.some(message=>message.type==='import-download')");
    assert.equal(await evaluate("window.importMessages.filter(message=>message.type==='import-download').length"),1);
    await evaluate("document.querySelector('#native-download').click()");
    assert.equal(await evaluate("window.importMessages.filter(message=>message.type==='import-download').length"),1, 'synthetic page clicks must not open the app');
    const nativePoint=await evaluate("(() => {const r=document.querySelector('#native-download').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()");
    await page('Input.dispatchMouseEvent',{type:'mousePressed',...nativePoint,button:'left',clickCount:1});
    await page('Input.dispatchMouseEvent',{type:'mouseReleased',...nativePoint,button:'left',clickCount:1});
    await wait("window.importMessages.filter(message=>message.type==='import-download').length===2");
    assert.ok(await evaluate("window.roots.some(root=>root.textContent.includes('見本B（共通素体）'))"));
    await evaluate("(() => {const s=window.roots[0].querySelector('select');s.value='a';s.dispatchEvent(new Event('change'));})()");
    assert.equal(await evaluate("document.querySelector('#b').hidden"),true);
    assert.equal(await evaluate("document.querySelector('#unknown').hidden"),false);
    await evaluate("window.offline=true;window.roots[0].querySelector('button').click()");
    await wait("window.roots[0].textContent.includes('アプリに接続できません')");
    assert.equal(await evaluate("document.querySelector('#b').hidden"),false);
    await evaluate("window.offline=false;window.roots[0].querySelector('button').click()");
    await wait("window.roots.some(root=>root.textContent.includes('所持未確認'))");
    // Exercise the actual popup script with browser DOM and an isolated runtime fixture.
    await page('Page.navigate',{url:base+'/extension/popup.html'});await wait("document.querySelector('#download-files') !== null");
    await evaluate(`(() => {
      window.started=[];window.chrome={
        runtime:{getManifest:()=>({version:'0.1.16'}),async sendMessage(message){window.started.push(message);return {ok:true};}},
        storage:{onChanged:{addListener(){}},local:{async get(defaults){return {...defaults,pairingToken:'ABCD2345',routeDownloads:true};},async set(){}}},
        tabs:{async query(){return [{id:1,url:'https://booth.pm/ja/items/123456'}];},async sendMessage(){return {products:[{boothId:'123456',title:'見本衣装',downloads:[{downloadableId:'555555',filename:'sample.zip',variation:'見本A用'}]}]};}}
      };
      window.fetch=async()=>({ok:true,status:200,json:async()=>({enabled:true})});
      ${companion}
      ${readFileSync('extension/request-policy.js','utf8')}
      ${readFileSync('extension/popup.js','utf8')}
    })()`);
    await wait("document.querySelector('#download-files button') !== null");
    assert.match(await evaluate("document.querySelector('#download-files button').textContent"),/見本A用/);
    const box=await evaluate("(() => {const node=document.querySelector('#download-files button');node.scrollIntoView({block:'center'});const r=node.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()");
    await page('Input.dispatchMouseEvent',{type:'mousePressed',...box,button:'left',clickCount:1});
    await page('Input.dispatchMouseEvent',{type:'mouseReleased',...box,button:'left',clickCount:1});
    await wait("window.started.some(message=>message.type==='start-download')");
    assert.equal(await evaluate("window.started.filter(message=>message.type==='start-download').length"),1);
    assert.match(await evaluate("document.querySelector('#status').textContent"),/取得を開始/);
  }finally{
    socket?.close();chrome.kill();await vite.close();await pause(200);await rm(profile,{recursive:true,force:true,maxRetries:3,retryDelay:100});
  }
});
