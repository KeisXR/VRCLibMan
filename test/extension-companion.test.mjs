import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import vm from 'node:vm';
import {webcrypto} from 'node:crypto';
import test from 'node:test';
const require=createRequire(import.meta.url);
const helper=require('../extension/companion.js');
const product={boothId:'123456',title:'テスト衣装',downloads:[{filename:'sample.zip',variation:'見本A用',downloadableId:'555555'}]};
function event(){const listeners=[];return {listeners,addListener(fn){listeners.push(fn);}};}
function worker(initial={}, previousItems=[]) {
  const data={pairingToken:'ABCD-2345',routeDownloads:true,...structuredClone(initial)};
  const items=structuredClone(previousItems);const sent=[];let online=true;let httpStatus=200;let rejectId='';
  const changes=event();const messages=event();const filenames=event();const alarms=event();const startup=event();const installed=event();
  const chrome={runtime:{id:'a'.repeat(32),onMessage:messages,onStartup:startup,onInstalled:installed,getManifest:()=>({version:'0.1.16'}),getURL:p=>`chrome-extension://${'a'.repeat(32)}/${p}`},
    storage:{local:{async get(defaults){return {...defaults,...structuredClone(data)};},async set(values){Object.assign(data,structuredClone(values));}},session:{async get(defaults){return defaults;}}},
    alarms:{onAlarm:alarms,create(){}},
    tabs:{async get(){return {url:'https://booth.pm/ja/items/123456'};},async sendMessage(){return {products:[product]};}},
    downloads:{onChanged:changes,onDeterminingFilename:filenames,
      async search(q){return items.filter(row=>q.id!=null?row.id===q.id:row.url===q.url);},
      async download(options){const id=items.length+1;const item={id,url:options.url,filename:`/Downloads/${options.filename}`,state:'in_progress',mime:'application/zip',fileSize:42};items.push(item);return id;}}
  };
  const context=vm.createContext({chrome,URL,AbortSignal,crypto:webcrypto,setTimeout,clearTimeout,
    async fetch(url,options){if(!online)throw new Error('offline');const body=JSON.parse(options.body);sent.push({url,body,options});const status=body.notificationId===rejectId?400:httpStatus;return {ok:status===200,status,async json(){return {ok:true,products:[]};}};}});
  context.importScripts=(...paths)=>{for(const p of paths)vm.runInContext(readFileSync(new URL(`../extension/${p}`,import.meta.url),'utf8'),context);};
  vm.runInContext(readFileSync(new URL('../extension/background.js',import.meta.url),'utf8'),context);
  return {data,items,sent,context,chrome,offline(){online=false;},online(){online=true;},status(value){httpStatus=value;},reject(value){rejectId=value;},
    async idle(){await vm.runInContext('serial',context);},
    async message(message,sender={id:chrome.runtime.id,url:chrome.runtime.getURL('popup.html')}){return new Promise(resolve=>{const result=messages.listeners[0](message,sender,resolve);if(result!==true)resolve(undefined);});},
    async finish(index=0){items[index].state='complete';changes.listeners[0]({id:items[index].id,state:{current:'complete'}});await vm.runInContext('serial',context);},
    async retry(){alarms.listeners[0]({name:'retry-deliveries'});await vm.runInContext('serial',context);}
  };
}
test('strict BOOTH URLs and exact unambiguous file mapping',()=>{
  assert.equal(helper.downloadId('https://booth.pm/downloadables/555555'),'555555');
  for(const url of ['https://booth.pm.evil.test/downloadables/555555','https://booth.pm@evil.test/downloadables/555555','http://booth.pm/downloadables/555555','https://booth.pm:444/downloadables/555555'])assert.equal(helper.downloadId(url),'');
  assert.equal(helper.choose([product],'555555').file.variation,'見本A用');
  assert.equal(helper.choose([product,product],'555555'),null);
  assert.equal(helper.choose([product],'123456'),null);
  assert.match(helper.labels({registered:true,owned:false,filesKnown:true}).join(' '),/所持未確認/);
  assert.equal(helper.matches(undefined,'avatar-a'),true);
  assert.equal(helper.matches({avatars:[{id:'b'}]},'a'),false);
});
test('only popup explicit action starts one download with canonical URL and staging path',async()=>{
  const w=worker();
  await w.message({type:'start-download',tabId:1,downloadableId:'555555'},{id:w.chrome.runtime.id,url:'https://booth.pm/ja/items/123456',tab:{id:1}});
  assert.equal(w.items.length,0);
  assert.equal((await w.message({type:'start-download',tabId:1,downloadableId:'555555'})).ok,true);
  assert.equal(w.items.length,1);assert.equal(w.items[0].url,'https://booth.pm/downloadables/555555');
  assert.match(w.items[0].filename,/VRCLibMan\/123456\/sample.zip$/);
  assert.equal(w.data.downloadIntents[0].file.variation,'見本A用');
  assert.match((await w.message({type:'start-download',tabId:1,downloadableId:'555555'})).error,/取得待ち/);
});
test('completion survives offline and worker restart, preserving exact variation and delivery ID',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});w.offline();await w.finish();
  assert.equal(w.data.pendingDeliveries.length,1);assert.equal(w.data.downloadIntents.length,0);
  const id=w.data.pendingDeliveries[0].id;
  const restarted=worker(w.data,w.items);await restarted.retry();
  assert.equal(restarted.data.pendingDeliveries.length,0);
  assert.equal(restarted.sent.length,2);
  assert.equal(restarted.sent[1].body.variation,'見本A用');
  assert.equal(restarted.sent[1].body.notificationId,id);
  for(const request of restarted.sent){assert.equal(request.options.credentials,'omit');assert.equal(request.options.headers.Cookie,undefined);}
});
test('non-2xx replies keep notification pending and token changes do not send to another pairing',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});w.status(400);await w.finish();
  assert.equal(w.data.pendingDeliveries.length,1);w.status(200);w.data.pairingToken='ZZZZ-9999';const count=w.sent.length;await w.retry();
  assert.equal(w.sent.length,count);assert.equal(w.data.pendingDeliveries.length,1);
});
test('one rejected notification does not block later deliveries',async()=>{
  const entries=['bad','good'].map(id=>({id,token:'ABCD2345',notice:{notificationId:id}}));
  const w=worker({pendingDeliveries:entries});w.reject('bad');await w.retry();
  assert.deepEqual(w.data.pendingDeliveries.map(row=>row.id),['bad']);
  assert.deepEqual(w.sent.map(row=>row.body.notificationId),['bad','good']);
  assert.match(w.data.deliveryError,/400/);
});
test('reconcile recovers complete downloads after shutdown before the completion event',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});w.items[0].state='complete';
  const restarted=worker(w.data,w.items);await restarted.retry();
  assert.equal(restarted.data.downloadIntents.length,0);assert.equal(restarted.sent[1].body.variation,'見本A用');
});
test('HTML login responses are not registered as asset files',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});w.items[0].mime='text/html';await w.finish();
  assert.equal(w.sent.length,0);assert.equal(w.data.pendingDeliveries?.length || 0,0);assert.match(w.data.deliveryError,/HTML/);
});
test('status queries require our BOOTH content script, credentials never go to content',async()=>{
  const w=worker();const sender={id:w.chrome.runtime.id,url:'https://booth.pm/ja/items/123456',tab:{id:1}};
  assert.equal((await w.message({type:'product-status',ids:['123456']},sender)).products.length,0);
  const invalid={...sender,url:'https://booth.pm.evil.test/'};
  assert.equal(await w.message({type:'product-status',ids:['123456']},invalid),undefined);
});

test('duplicate complete events keep one stable receipt even after successful delivery',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});await w.finish();
  const first=w.sent.find(row=>row.url.endsWith('/download-finished')).body.notificationId;
  await w.finish();
  const notices=w.sent.filter(row=>row.url.endsWith('/download-finished'));
  assert.equal(notices[1].body.notificationId,first);
});
test('native user click mapping follows filename determination without starting another download',async()=>{
  const w=worker();const sender={id:w.chrome.runtime.id,url:'https://booth.pm/ja/items/123456',tab:{id:1}};
  await w.message({type:'remember-download',downloadableId:'555555'},sender);
  assert.equal(w.items.length,0);
  const item={id:42,url:'https://booth.pm/downloadables/555555',filename:'/Downloads/sample.zip',state:'in_progress',mime:'application/zip'};
  let decision;
  await new Promise(resolve=>w.chrome.downloads.onDeterminingFilename.listeners[0](item,result=>{decision=result;resolve();}));
  await w.idle();
  assert.equal(decision.filename,'VRCLibMan/123456/sample.zip');
  w.items.push({...item,filename:`/Downloads/${decision.filename}`});
  await w.finish();
  assert.equal(w.sent[1].body.variation,'見本A用');
});

test('canceled downloads release their mapping so the user can retry explicitly',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});
  w.items[0].state='interrupted';w.items[0].canResume=false;await w.retry();
  assert.equal(w.data.downloadIntents.length,0);assert.equal(w.sent.length,0);
  assert.equal((await w.message({type:'start-download',tabId:1,downloadableId:'555555'})).ok,true);
  assert.equal(w.items.length,2);
});
test('resumable interruptions retain correlation without an automatic redownload',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});
  w.items[0].state='interrupted';w.items[0].canResume=true;await w.retry();
  assert.equal(w.data.downloadIntents.length,1);assert.equal(w.items.length,1);assert.equal(w.sent.length,0);
  await w.finish();assert.equal(w.sent[1].body.variation,'見本A用');
});
