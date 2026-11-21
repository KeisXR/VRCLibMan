import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import test from 'node:test';
const require=createRequire(import.meta.url);
const library=require('../extension/library.js');
const shared=require('../extension/shared.js');
const source=file=>readFileSync(`extension/${file}`,'utf8');

const avatars=[{id:'lime',displayName:'ライム'},{id:'plum',displayName:'プラム'},{id:'manuka',displayName:'マヌカ'}];

test('the avatars of the page being viewed come first, and the rest keep the app\'s order', ()=>{
  const ordered=library.orderAvatars(avatars,['manuka','lime','gone']);
  assert.deepEqual(ordered.map(a=>[a.id,a.onPage]),[['manuka',true],['lime',true],['plum',false]]);
  // Opted out (no page avatars passed): the app's order, nothing marked.
  assert.deepEqual(library.orderAvatars(avatars,[]).map(a=>[a.id,a.onPage]),[['lime',false],['plum',false],['manuka',false]]);
  assert.deepEqual(library.orderAvatars(null,['lime']),[]);
});

test('the first selection is the saved one while it still exists, else the first avatar, else everything', ()=>{
  assert.equal(library.defaultScope(avatars,'avatar:plum'),'avatar:plum');
  assert.equal(library.defaultScope(avatars,'kind:world'),'kind:world');
  assert.equal(library.defaultScope(avatars,'avatar:deleted'),'avatar:lime');
  assert.equal(library.defaultScope(avatars,''),'avatar:lime');
  assert.equal(library.defaultScope([],'avatar:lime'),'kind:asset');
  assert.equal(library.defaultScope(avatars,'kind:other'),'avatar:lime');
});

test('the filter matches every word against the name and the shop, ignoring width and case', ()=>{
  const sections=[
    {key:'outfit',label:'衣装',items:[{title:'ライム用ワンピース',shop:'Sample Shop'},{title:'パーカー',shop:'別の店'}]},
    {key:'hair',label:'髪型',items:[{title:'ショートヘア',shop:'Sample Shop'}]},
  ];
  assert.equal(library.filterSections(sections,'  '),sections);
  const one=library.filterSections(sections,'ＳＡＭＰＬＥ ワンピ');
  assert.deepEqual(one.map(s=>[s.key,s.items.length]),[['outfit',1]]);
  assert.deepEqual(library.filterSections(sections,'sample').map(s=>s.key),['outfit','hair']);
  assert.deepEqual(library.filterSections(sections,'無い語'),[]);
});

test('links are built from numeric IDs only, and dates show the day without the time', ()=>{
  assert.equal(shared.itemUrl('123456'),'https://booth.pm/ja/items/123456');
  assert.equal(shared.itemUrl('12/../x'),'');
  assert.equal(shared.orderUrl('12345678'),'https://accounts.booth.pm/orders/12345678');
  assert.equal(shared.orderUrl('javascript:1'),'');
  assert.equal(shared.formatDate('2026-03-04T21:30:00+09:00'),'2026/3/4');
  assert.equal(shared.formatDate(null),'');
  assert.equal(shared.DEFAULTS.followPageAvatar,true);
});

test('the side panel takes names and thumbnails from the app, and never from BOOTH\'s image servers', ()=>{
  const panel=source('library.js')+source('sidepanel.js');
  assert.match(panel,/\$\{shared\.LOCAL_BASE\}\/library\/thumb/);
  assert.match(panel,/credentials: "omit"/);
  assert.doesNotMatch(panel,/pximg|innerHTML|chrome\.cookies/);
  // Product names reach the page only through textContent (the `text` prop of ui.h).
  assert.doesNotMatch(source('pagecards.js')+panel,/insertAdjacentHTML|outerHTML|document\.write/);
  // Scripts that run inside BOOTH pages never ask for the library.
  const manifest=JSON.parse(source('manifest.json'));
  for(const file of new Set(['extract.js',...manifest.content_scripts.flatMap(entry=>entry.js)])){
    assert.doesNotMatch(source(file),/library-(avatars|items|product)|\/library\/(avatars|items|thumb|product)/,file);
  }
  const html=source('sidepanel.html');
  for(const file of ['shared.js','companion.js','ui.js','pagecards.js','library.js','sidepanel.js','sidepanel.css']) assert.ok(html.includes(`"${file}"`),file);
  assert.ok(source('popup.html').includes('"pagecards.js"'));
});

test('the app only follows an extension request to a product in the lists or to an avatar', async()=>{
  const {createServer}=await import('vite');
  const server=await createServer({server:{middlewareMode:true,hmr:false},appType:'custom',logLevel:'error'});
  try{
    const {extensionRoute}=await server.ssrLoadModule('/src/extensionRoute.ts');
    for(const route of ['#/assets/item/123456','#/worlds/item/1234','#/avatar/lime','#/avatar/owned%206000%2F%E3%81%82']) assert.equal(extensionRoute(route),route);
    for(const route of ['#/settings','#/import','#/assets/item/12','#/assets/item/123456/x','#/avatar/','#/avatar/a/b','#/avatar/a?x=1','javascript:alert(1)','https://example.com/#/avatar/a','',null,42]) assert.equal(extensionRoute(route),null,String(route));
  }finally{await server.close();}
  const rust=source('../src-tauri/src/local_import.rs');
  assert.match(rust,/Reply::Open \{ route \}/);
  assert.match(source('../src/App.tsx'),/useExtensionRoute\(\)/);
  assert.match(source('../src/preview/api.ts'),/case "take_extension_route"/);
});
