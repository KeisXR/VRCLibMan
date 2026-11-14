import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import vm from 'node:vm';
import {webcrypto} from 'node:crypto';
import test from 'node:test';
const require=createRequire(import.meta.url);
const helper=require('../extension/companion.js');
const product={boothId:'123456',title:'テスト衣装',downloads:[{filename:'sample.zip',variation:'見本A用',downloadableId:'555555'}]};
const EXT='a'.repeat(32);
const page=(url='https://booth.pm/ja/items/123456')=>({id:EXT,url,tab:{id:1}});
const popup={id:EXT,url:`chrome-extension://${EXT}/popup.html`};
const offscreenPage={id:EXT,url:`chrome-extension://${EXT}/offscreen.html`};
function event(){const listeners=[];return {listeners,addListener(fn){listeners.push(fn);}};}
// Requests other than the periodic version check, which may interleave with retries.
const local=w=>w.sent.filter(row=>row.path!=='/health');

// Service worker under test with an in-memory browser. Local requests are answered by options.app.
function worker(initial={}, previousItems=[], options={}) {
  const data={pairingToken:'ABCD-2345',routeDownloads:true,...structuredClone(initial)};
  const session={...structuredClone(options.session||{})};
  const items=structuredClone(previousItems);const sent=[];const opened=[];const removed=[];const offscreenMessages=[];const savedFiles=[];const reloads=[];
  let online=options.online!==false;let httpStatus=200;let rejectId='';let offscreen=false;
  const app=options.app||{};
  const changes=event();const messages=event();const filenames=event();const alarms=event();const startup=event();const installed=event();
  const area=(store)=>({async get(defaults){const base=typeof defaults==='string'?{[defaults]:undefined}:defaults;return {...base,...structuredClone(store)};},async set(values){Object.assign(store,structuredClone(values));}});
  const chrome={runtime:{id:EXT,onMessage:messages,onStartup:startup,onInstalled:installed,getManifest:()=>({version:'0.8.3'}),getURL:p=>`chrome-extension://${EXT}/${p}`,reload(){reloads.push(true);},
      async getContexts(){return offscreen?[{contextType:'OFFSCREEN_DOCUMENT'}]:[];},
      async sendMessage(message){offscreenMessages.push(message);if(!offscreen)throw new Error('Receiving end does not exist.');
        if(message.type==='blob-url')return {url:'blob:chrome-extension://test/1'};
        if(message.type==='status')return {active:options.offscreenActive||null};
        if(message.type==='walk-cancel')return {ok:true};
        if(message.type==='order-detail')return {value:{href:`https://accounts.booth.pm/orders/${message.orderId}`,detail:{orderId:message.orderId}}};
        return {ok:true};}},
    offscreen:{async createDocument(){offscreen=true;},async closeDocument(){offscreen=false;}},
    storage:{local:area(data),session:area(session)},
    alarms:{onAlarm:alarms,create(){}},
    scripting:{async executeScript(){return [{result:[product]}];}},
    tabs:{async get(){return {id:1,url:options.tabUrl||'https://booth.pm/ja/items/123456'};},async sendMessage(){return {products:options.products||[product]};},
      async create(options){opened.push(options);if(app.wakeStartsApp)online=true;return {id:99};},async remove(id){removed.push(id);}},
    downloads:{onChanged:changes,onDeterminingFilename:filenames,
      async search(q){return items.filter(row=>q.id!=null?row.id===q.id:row.url===q.url);},
      async download(options){if(options.url.startsWith('blob:')){savedFiles.push(options);return 900;}const id=items.length+1;const item={id,url:options.url,filename:`/Downloads/${options.filename}`,state:'in_progress',mime:'application/zip',fileSize:42};items.push(item);return id;}}
  };
  const context=vm.createContext({chrome,URL,AbortSignal,crypto:webcrypto,setTimeout:(fn,ms)=>setTimeout(fn,Math.min(ms,5)),clearTimeout,Date,
    async fetch(url,options){if(!online)throw new TypeError('Failed to fetch');const body=options.body ? JSON.parse(options.body) : {};const path=url.replace('http://127.0.0.1:17321','');sent.push({url,path,body,options});
      const status=body.notificationId===rejectId?400:httpStatus;
      const reply=app[path]?app[path](body):{ok:true,products:[]};
      return {ok:status===200,status,async json(){return reply;}};}});
  context.importScripts=(...paths)=>{for(const p of paths)vm.runInContext(readFileSync(new URL(`../extension/${p}`,import.meta.url),'utf8'),context);};
  vm.runInContext(readFileSync(new URL('../extension/background.js',import.meta.url),'utf8'),context);
  return {data,session,items,sent,opened,removed,offscreenMessages,savedFiles,reloads,context,chrome,
    offline(){online=false;},online(){online=true;},status(value){httpStatus=value;},reject(value){rejectId=value;},
    async idle(){await vm.runInContext('serial',context);await vm.runInContext('jobSerial',context);},
    async message(message,sender=popup){return new Promise(resolve=>{const result=messages.listeners[0](message,sender,resolve);if(result!==true)resolve(undefined);});},
    async finish(index=0){items[index].state='complete';changes.listeners[0]({id:items[index].id,state:{current:'complete'}});await vm.runInContext('serial',context);},
    async retry(){alarms.listeners[0]({name:'retry-deliveries'});await vm.runInContext('serial',context);}
  };
}

test('a page download imports one product without opening a launch tab when the app is running', async()=>{
  const w=worker({routeDownloads:false});
  assert.equal((await w.message({type:'import-download',downloadableId:'555555'},page())).ok,true);
  assert.equal(w.opened.length,0);
  assert.deepEqual(w.sent.map(r=>r.path),['/health','/import-download']);
  assert.deepEqual(w.sent[1].body,product);
  assert.equal(w.items.length,0);
  for(const request of w.sent){assert.equal(request.options.credentials,'omit');assert.equal(request.options.headers.Cookie,undefined);assert.equal(request.options.headers['X-VRCLibMan-Extension-Version'],'0.8.3');assert.equal(request.options.method,'POST',`${request.path} is sent as POST so that Chrome adds Origin`);}
  assert.equal(await w.message({type:'import-download',downloadableId:'555555'},{...page(),url:'https://evil.example/'}),undefined);
});

test('a stopped app is woken with a visible launch tab, which closes once the app answers', async()=>{
  const w=worker({routeDownloads:false},[],{online:false,app:{wakeStartsApp:true}});
  assert.equal((await w.message({type:'import-download',downloadableId:'555555'},page())).ok,true);
  assert.deepEqual(structuredClone(w.opened),[{url:'vrclibman://import',active:true}]);
  assert.deepEqual(w.removed,[99]);
  assert.deepEqual(w.sent.map(r=>r.path),['/health','/import-download']);
});

test('ambiguous file or missing pairing does not launch the app or import', async()=>{
  for (const [initial,id] of [[{pairingToken:''},'555555'],[{},'999999']]) {
    const w=worker(initial);
    assert.ok((await w.message({type:'import-download',downloadableId:id},page())).error);
    assert.equal(w.opened.length,0);assert.equal(w.sent.length,0);
  }
});

test('registering a product from the library sends exactly that product', async()=>{
  const other={boothId:'234567',title:'別の商品'};
  const w=worker({},[],{products:[other,product],tabUrl:'https://accounts.booth.pm/library'});
  const sender=page('https://accounts.booth.pm/library');
  assert.equal((await w.message({type:'import-product',boothId:'234567'},sender)).ok,true);
  assert.deepEqual(w.sent.at(-1).body,other);
  assert.ok((await w.message({type:'import-product',boothId:'999999'},sender)).error);
});

test('strict BOOTH URLs, exact file mapping and short state marks',()=>{
  assert.equal(helper.downloadId('https://booth.pm/downloadables/555555'),'555555');
  for(const url of ['https://booth.pm.evil.test/downloadables/555555','https://booth.pm@evil.test/downloadables/555555','http://booth.pm/downloadables/555555','https://booth.pm:444/downloadables/555555'])assert.equal(helper.downloadId(url),'');
  assert.equal(helper.choose([product],'555555').file.variation,'見本A用');
  assert.equal(helper.choose([product,product],'555555'),null);
  assert.equal(helper.choose([product],'123456'),null);
  assert.deepEqual(helper.labels({registered:true,owned:false,filesKnown:true}),['登録のみ','保存記録なし']);
  assert.deepEqual(helper.labels({registered:true,owned:true,localRecorded:true}),['所持','保存記録あり']);
  assert.deepEqual(helper.labels(undefined),['未登録']);
  assert.deepEqual(helper.avatarNames({avatars:[{name:'A',viaBaseBody:false},{name:'B',viaBaseBody:true}]}),['A','B・共通素体']);
  assert.equal(helper.saved({savedFiles:['555555']},'555555'),true);
  assert.equal(helper.matches(undefined,'avatar-a'),true);
  assert.equal(helper.matches({avatars:[{id:'b'}]},'a'),false);
});

test('only an explicit file button starts one download, from our popup or our page button',async()=>{
  const w=worker();
  assert.equal(await w.message({type:'start-download',tabId:1,downloadableId:'555555'},{id:EXT,url:'https://evil.example/'}),undefined);
  assert.equal(w.items.length,0);
  assert.equal((await w.message({type:'start-download',tabId:1,downloadableId:'555555'})).ok,true);
  assert.equal(w.items.length,1);assert.equal(w.items[0].url,'https://booth.pm/downloadables/555555');
  assert.match(w.items[0].filename,/VRCLibMan\/123456\/sample.zip$/);
  assert.equal(w.data.downloadIntents[0].file.variation,'見本A用');
  assert.match((await w.message({type:'start-download',downloadableId:'555555'},page())).error,/取得待ち/);
  const fromPage=worker();
  assert.equal((await fromPage.message({type:'start-download',downloadableId:'555555'},page())).ok,true);
  assert.equal(fromPage.items.length,1);
});

test('completion survives offline and worker restart, preserving exact variation and delivery ID',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});w.offline();await w.finish();
  assert.equal(w.data.pendingDeliveries.length,1);assert.equal(w.data.downloadIntents.length,0);
  assert.equal(w.data.downloadLog[0].state,'pending');
  const id=w.data.pendingDeliveries[0].id;
  const restarted=worker(w.data,w.items);await restarted.retry();
  assert.equal(restarted.data.pendingDeliveries.length,0);
  assert.deepEqual(local(restarted).map(r=>r.path),['/import','/download-finished']);
  assert.equal(local(restarted)[1].body.variation,'見本A用');
  assert.equal(local(restarted)[1].body.notificationId,id);
  assert.equal(restarted.data.downloadLog[0].state,'done');
  for(const request of restarted.sent){assert.equal(request.options.credentials,'omit');assert.equal(request.options.headers.Cookie,undefined);}
});
test('non-2xx replies keep notification pending and token changes do not send to another pairing',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});w.status(400);await w.finish();
  assert.equal(w.data.pendingDeliveries.length,1);w.status(200);w.data.pairingToken='ZZZZ-9999';const count=local(w).length;await w.retry();
  assert.equal(local(w).length,count);assert.equal(w.data.pendingDeliveries.length,1);
});
test('one rejected notification does not block later deliveries',async()=>{
  const entries=['bad','good'].map(id=>({id,token:'ABCD2345',notice:{notificationId:id}}));
  const w=worker({pendingDeliveries:entries});w.reject('bad');await w.retry();
  assert.deepEqual(w.data.pendingDeliveries.map(row=>row.id),['bad']);
  assert.deepEqual(local(w).map(row=>row.body.notificationId),['bad','good']);
  assert.match(w.data.deliveryError,/400/);
});
test('reconcile recovers complete downloads after shutdown before the completion event',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});w.items[0].state='complete';
  const restarted=worker(w.data,w.items);await restarted.retry();
  assert.equal(restarted.data.downloadIntents.length,0);assert.equal(local(restarted)[1].body.variation,'見本A用');
});
test('HTML login responses are not registered as asset files',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});w.items[0].mime='text/html';await w.finish();
  assert.equal(w.sent.length,0);assert.equal(w.data.pendingDeliveries?.length || 0,0);assert.match(w.data.deliveryError,/HTML/);
});
test('status queries come from our BOOTH page or our own pages, and the token never goes to the page',async()=>{
  const w=worker({},[],{app:{'/product-status':()=>({products:[{boothId:'123456',registered:true}]})}});
  const answer=await w.message({type:'product-status',ids:['123456']},page());
  assert.equal(answer.products.length,1);
  assert.equal(JSON.stringify(answer).includes('ABCD'),false);
  assert.equal(await w.message({type:'product-status',ids:['123456']},{...page(),url:'https://booth.pm.evil.test/'}),undefined);
  assert.equal((await w.message({type:'product-status',ids:['123456']})).products.length,1);
  const state=await w.message({type:'page-state'},page());
  assert.deepEqual(structuredClone(state),{paired:true,routeDownloads:true,showBadges:true,autoPurchase:true});
});

test('duplicate complete events keep one stable receipt even after successful delivery',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});await w.finish();
  const first=w.sent.find(row=>row.path==='/download-finished').body.notificationId;
  await w.finish();
  const notices=w.sent.filter(row=>row.path==='/download-finished');
  assert.equal(notices[1].body.notificationId,first);
});
test('native user click mapping follows filename determination without starting another download',async()=>{
  const w=worker();
  await w.message({type:'remember-download',downloadableId:'555555'},page());
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

test('page notes from the content script reach the worker and title unmatched routed downloads',async()=>{
  const w=worker();
  await w.message({type:'page-notes',notes:[{boothId:'123456',title:'メモの題',isFree:true},{boothId:'../1',title:'x'}]},page());
  await w.idle();
  assert.deepEqual(w.session.pageNotes.map(note=>note.boothId),['123456']);
  w.items.push({id:7,url:'https://booth.pm/downloadables/777777',referrer:'https://booth.pm/ja/items/123456',filename:'/Downloads/VRCLibMan/123456/free.zip',state:'complete',mime:'application/zip',fileSize:10});
  await w.finish(0);
  const notice=w.sent.find(row=>row.path==='/download-finished').body;
  assert.equal(notice.title,'メモの題');
  assert.equal(notice.isFree,true);
});

test('canceled downloads release their mapping so the user can retry explicitly',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});
  w.items[0].state='interrupted';w.items[0].canResume=false;await w.retry();
  assert.equal(w.data.downloadIntents.length,0);assert.equal(w.sent.filter(row=>row.path!=='/health').length,0);
  assert.equal((await w.message({type:'start-download',tabId:1,downloadableId:'555555'})).ok,true);
  assert.equal(w.items.length,2);
});
test('resumable interruptions retain correlation without an automatic redownload',async()=>{
  const w=worker();await w.message({type:'start-download',tabId:1,downloadableId:'555555'});
  w.items[0].state='interrupted';w.items[0].canResume=true;await w.retry();
  assert.equal(w.data.downloadIntents.length,1);assert.equal(w.items.length,1);assert.equal(w.sent.filter(row=>row.path!=='/health').length,0);
  await w.finish();assert.equal(w.sent.find(row=>row.path==='/download-finished').body.variation,'見本A用');
});

const checkout={type:'checkout',href:'https://checkout.booth.pm/orders/90000003',orderId:'90000003',products:[{boothId:'123456',title:'見本の衣装',shop:'見本の店'},{boothId:'abc',title:'壊れた商品'}]};
const checkoutPage={id:EXT,url:'https://checkout.booth.pm/orders/90000003',tab:{id:2}};

test('a checkout page is registered once, then its order detail is read without the page',async()=>{
  const w=worker({},[],{app:{'/purchases/checkout':()=>({recorded:true,registered:1,needsDetail:true})}});
  await w.chrome.offscreen.createDocument();
  await w.message(checkout,checkoutPage);
  const posted=w.sent.find(row=>row.path==='/purchases/checkout');
  assert.deepEqual(posted.body.products.map(row=>row.boothId),['123456']);
  assert.equal(posted.body.orderId,'90000003');
  for(let i=0;i<20&&!w.sent.some(row=>row.path==='/purchases/detail');i++)await new Promise(r=>setTimeout(r,5));
  assert.ok(w.offscreenMessages.some(m=>m.type==='order-detail'&&m.orderId==='90000003'));
  assert.equal(w.sent.find(row=>row.path==='/purchases/detail').body.href,'https://accounts.booth.pm/orders/90000003');
  assert.equal(w.data.pendingDeliveries.length,0);
  const count=w.sent.length;
  await w.message(checkout,checkoutPage);
  assert.equal(w.sent.length,count,'the same order is not sent twice');
});

test('a checkout waits for the app and is not read when automatic registration is off or the page does not match',async()=>{
  const w=worker({},[],{online:false});
  await w.message(checkout,checkoutPage);
  assert.equal(w.data.pendingDeliveries[0].kind,'checkout');
  w.online();await w.retry();
  assert.equal(w.data.pendingDeliveries.length,0);
  const off=worker({autoPurchase:false});
  await off.message(checkout,checkoutPage);
  assert.equal(off.sent.length,0);
  const elsewhere=worker();
  assert.equal((await elsewhere.message(checkout,{...checkoutPage,url:'https://checkout.booth.pm/orders/90000004'})).ok,false);
  assert.equal((await elsewhere.message({...checkout,href:'https://booth.pm/orders/90000003'},{...checkoutPage,url:'https://booth.pm/orders/90000003'})).ok,false);
  assert.equal(elsewhere.sent.length,0);
});

test('an order history page the user opened is passed on once, only while registration is on',async()=>{
  const w=worker();
  const sender={id:EXT,url:'https://accounts.booth.pm/orders?page=2',tab:{id:3}};
  await w.message({type:'orders-page',kind:'list',href:'https://accounts.booth.pm/orders?page=2',orders:[{orderId:'90000001'}]},sender);
  assert.equal(w.sent.at(-1).path,'/purchases/list');
  assert.deepEqual(w.sent.at(-1).body.orders,[{orderId:'90000001'}]);
  const off=worker({autoPurchase:false});
  await off.message({type:'orders-page',kind:'list',href:'https://accounts.booth.pm/orders',orders:[]},{...sender,url:'https://accounts.booth.pm/orders'});
  assert.equal(off.sent.length,0);
});

test('a new-purchase job asks for known IDs, hands the walk to the offscreen document and delivers the result',async()=>{
  const w=worker({},[],{app:{'/known-ids':()=>({ids:['9001']}),'/import':(body)=>({inserted:body.products.length,updated:0,unchanged:0})}});
  const started=await w.message({type:'job-start',kind:'library-new'});
  assert.equal(started.ok,true);
  const walk=w.offscreenMessages.find(m=>m.type==='walk');
  assert.equal(walk.target,'offscreen');
  assert.deepEqual(walk.job.knownIds,['9001']);
  assert.equal(walk.job.token,undefined,'library walks do not carry the code');
  assert.equal(w.session.job.state,'running');
  await w.message({type:'walk-progress',id:started.id,line:'購入ライブラリ 2 / 3 ページ',current:2,total:3,count:4},offscreenPage);
  assert.equal(w.session.job.current,2);
  await w.message({type:'walk-result',id:started.id,kind:'library-new',products:[{boothId:'9003',title:'新しい服'}],pages:3,stopped:true,complete:false},offscreenPage);
  await w.idle();
  const imported=w.sent.find(row=>row.path==='/import').body;
  assert.equal(imported.source,'library-update');
  assert.equal(imported.complete,undefined);
  assert.equal(w.session.job.state,'done');
  assert.match(w.session.job.line,/追加 1 件.*3 ページを確認.*そこまでで止めました/);
  assert.equal(w.data.importHistory[0].kind,'library-new');
  await w.message({type:'walk-progress',id:started.id,line:'遅れて届いた進み具合'},offscreenPage);
  assert.equal(w.session.job.state,'done','a late progress note never revives a finished job');
  assert.equal(await w.message({type:'walk-result',id:started.id,kind:'library',products:[]},page()),undefined,'only the offscreen document reports results');
});

test('a finished full library walk is sent as a complete file list; without pairing it is saved as JSON',async()=>{
  const w=worker();
  const started=await w.message({type:'job-start',kind:'library'});
  await w.message({type:'walk-result',id:started.id,kind:'library',products:[{boothId:'9003'}],pages:5,complete:true},offscreenPage);
  const imported=w.sent.find(row=>row.path==='/import').body;
  assert.equal(imported.complete,true);
  assert.equal(imported.source,'library');
  const unpaired=worker({pairingToken:''});
  const job=await unpaired.message({type:'job-start',kind:'library'});
  await unpaired.message({type:'walk-result',id:job.id,kind:'library',products:[{boothId:'9003'}],pages:1,complete:true},offscreenPage);
  assert.equal(unpaired.sent.length,0);
  assert.equal(unpaired.savedFiles[0].saveAs,true);
  assert.match(unpaired.savedFiles[0].filename,/^vrclibman-booth-.*\.json$/);
  assert.match(unpaired.session.job.line,/JSON を保存しました/);
});

test('a purchase-history job needs the app, passes its own code to the walker and records the summary',async()=>{
  const w=worker({},[],{app:{'/purchases/state':()=>({complete:true})}});
  const started=await w.message({type:'job-start',kind:'purchases'});
  const walk=w.offscreenMessages.find(m=>m.type==='walk');
  assert.equal(walk.job.complete,true);
  assert.equal(walk.job.token,'ABCD-2345');
  assert.equal(walk.job.version,'0.8.3');
  await w.message({type:'walk-result',id:started.id,kind:'purchases',purchases:{pages:2,added:3,details:3,notice:''}},offscreenPage);
  assert.equal(w.session.job.state,'done');
  assert.match(w.session.job.line,/購入履歴 2 ページ、新しい注文 3 件、詳細 3 件/);
  const unpaired=worker({pairingToken:''});
  assert.match((await unpaired.message({type:'job-start',kind:'purchases'})).error,/ペアリング/);
  assert.equal(unpaired.session.job.state,'error');
});

test('cancel reaches the walker, and a job left running without a walker is closed as interrupted',async()=>{
  const w=worker();
  const started=await w.message({type:'job-start',kind:'library'});
  await w.message({type:'job-cancel'});
  assert.ok(w.offscreenMessages.some(m=>m.type==='walk-cancel'&&m.id===started.id));
  const stale={id:'old',kind:'library',label:'x',state:'running',line:'',current:0,total:0,count:0,startedAt:new Date(Date.now()-60000).toISOString()};
  const restarted=worker({},[],{session:{job:stale}});
  await restarted.message({type:'connection'});
  await restarted.idle();
  for(let i=0;i<20&&restarted.session.job.state==='running';i++)await new Promise(r=>setTimeout(r,5));
  assert.equal(restarted.session.job.state,'error');
  assert.match(restarted.session.job.line,/中断/);
});


const settle=async()=>{for(let i=0;i<30;i++)await new Promise(r=>setTimeout(r,2));};

test('the extension reloads itself once into a newer version the app placed, and stops if the reload changed nothing',async()=>{
  const app={'/health':()=>({ok:true,extension:'0.9.0'})};
  const w=worker({},[],{app});
  await w.retry();await settle();
  assert.equal(w.reloads.length,1);
  assert.equal(w.data.reloadedFor,'0.9.0');
  // Still 0.8.3 after the reload: the extension is loaded from another folder.
  const again=worker({...w.data,lastVersionCheck:0},[],{app});
  await again.retry();await settle();
  assert.equal(again.reloads.length,0,'no reload loop');
  assert.deepEqual(structuredClone(again.data.extensionUpdate),{version:'0.9.0',stuck:true});
  const checked=again.sent.filter(row=>row.path==='/health').length;
  await again.retry();await settle();
  assert.equal(again.sent.filter(row=>row.path==='/health').length,checked,'the app is asked at most every ten minutes');
});

test('a running walk delays the reload, and opening the popup only offers it',async()=>{
  const app={'/health':()=>({ok:true,extension:'0.9.0'})};
  const running={id:'job',kind:'library',label:'x',state:'running',line:'',current:0,total:0,count:0,startedAt:new Date().toISOString()};
  const busy=worker({},[],{app,session:{job:running},offscreenActive:'job'});
  await busy.retry();await settle();
  assert.equal(busy.reloads.length,0);
  assert.deepEqual(structuredClone(busy.data.extensionUpdate),{version:'0.9.0',stuck:false});
  assert.match((await busy.message({type:'reload-extension'})).error,/読み込みが終わってから/);
  const idle=worker({},[],{app});
  await idle.message({type:'connection'});
  assert.equal(idle.reloads.length,0,'opening the popup does not close it by reloading');
  assert.equal(idle.data.extensionUpdate.version,'0.9.0');
  await idle.message({type:'reload-extension'});
  assert.equal(idle.reloads.length,1);
});

test('the same or an older version in the app folder never reloads the extension',async()=>{
  for(const extension of ['0.8.3','0.8.2',null,'1.0.0-beta']){
    const w=worker({extensionUpdate:{version:'0.9.0',stuck:true}},[],{app:{'/health':()=>({ok:true,extension})}});
    await w.retry();await settle();
    assert.equal(w.reloads.length,0,String(extension));
    assert.equal(w.data.extensionUpdate,null);
  }
});
