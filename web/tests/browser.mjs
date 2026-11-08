import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import http from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const captures = path.join(root, 'screenshots');
const site = {
  site_name: 'MessagePusher',
  site_description: '一个接口，发送到你需要的每个平台。',
  announcement: '欢迎使用消息通知平台。',
  allow_registration: true
};
const admin = { id: '00000000-0000-4000-8000-000000000001', username: 'administrator', role: 'admin', active: true };
const regular = { id: '00000000-0000-4000-8000-000000000002', username: 'newmember', role: 'user', active: true };
const providers = [];
const keys = [];
const events = [];
const json = (res, status, data) => {
  res.writeHead(status, {'content-type': 'application/json', 'cache-control': 'no-store'});
  res.end(JSON.stringify(data));
};
function bodyOf(req) {
  return new Promise((resolve,reject)=>{
    const chunks=[];
    req.on('data',chunk=>chunks.push(chunk));
    req.on('error',reject);
    req.on('end',()=>{
      try{resolve(chunks.length?JSON.parse(Buffer.concat(chunks).toString('utf8')):{});}
      catch(error){reject(error);}
    });
  });
}
const server = http.createServer(async(req,res)=>{
  try{
    const url = new URL(req.url,'http://localhost');
    if(req.method==='GET' && ['/','/assets/styles.css','/assets/app.js'].includes(url.pathname)){
      const source = {'/':'index.html','/assets/styles.css':'styles.css','/assets/app.js':'app.js'}[url.pathname];
      const payload = await readFile(path.join(root,source));
      res.writeHead(200,{'content-type':source.endsWith('.html')?'text/html':source.endsWith('.css')?'text/css':'application/javascript'});
      res.end(payload);return;
    }
    if(req.method==='GET' && url.pathname==='/v1/site'){json(res,200,site);return;}
    if(req.method==='POST' && url.pathname==='/v1/login'){json(res,200,{token:'admin-session'});return;}
    if(req.method==='POST' && url.pathname==='/v1/register'){
      if(!site.allow_registration){json(res,403,{error:'Permission denied'});return;}
      const body=await bodyOf(req);
      regular.username=body.username;
      json(res,201,{id:regular.id,role:'user',token:'user-session'});return;
    }
    const user = req.headers.authorization==='Bearer user-session'?regular:admin;
    if(req.method==='GET' && url.pathname==='/v1/me'){json(res,200,user);return;}
    if(req.method==='GET' && url.pathname==='/v1/providers'){json(res,200,user.role==='admin'?providers:[]);return;}
    if(req.method==='GET' && url.pathname==='/v1/keys'){json(res,200,user.role==='admin'?keys:[]);return;}
    if(req.method==='GET' && url.pathname==='/v1/events'){json(res,200,user.role==='admin'?events:[]);return;}
    if(req.method==='GET' && url.pathname==='/v1/users'){json(res,200,[admin,regular]);return;}
    if(req.method==='GET' && url.pathname==='/v1/admin/overview'){json(res,200,{active_users:2,enabled_providers:providers.length,events_last_24h:0,failed_deliveries:0});return;}
    if(req.method==='POST' && url.pathname==='/v1/providers'){
      const value=await bodyOf(req);
      const id='00000000-0000-4000-8000-000000000003';
      providers.push({id,name:value.name,kind:value.kind,enabled:true,subscribed:Boolean(value.settings.chat_id),delivery:'direct',created_at:new Date().toISOString()});
      json(res,201,{id});return;
    }
    if(req.method==='GET' && url.pathname.startsWith('/v1/providers/')){
      const provider=providers.find(p=>p.id===url.pathname.split('/').at(-1));
      json(res,200,{...provider,settings:{chat_id:'12345'},stored_secrets:['bot_token'],delivery:{mode:'direct'}});return;
    }
    if(req.method==='PATCH' && url.pathname.startsWith('/v1/providers/')){
      const change=await bodyOf(req);
      const provider=providers.find(p=>p.id===url.pathname.split('/').at(-1));
      Object.assign(provider,change);
      json(res,200,{updated:true});return;
    }
    if(req.method==='POST' && url.pathname==='/v1/keys'){
      const value=await bodyOf(req);
      keys.push({id:'00000000-0000-4000-8000-000000000004',name:value.name,key_prefix:'mpk_123',created_at:new Date().toISOString()});
      json(res,201,{key:'mpk_test_key_for_browser_preview',id:keys[0].id});return;
    }
    if(req.method==='POST' && url.pathname==='/v1/send'){
      const value=await bodyOf(req);
      const id='00000000-0000-4000-8000-000000000005';
      events.unshift({id,title:value.title,created_at:new Date().toISOString(),total:providers.length,sent:0,failed:0});
      json(res,202,{id,status:'queued'});return;
    }
    if(req.method==='GET' && url.pathname.startsWith('/v1/events/')){
      const id=url.pathname.split('/').at(-1);
      json(res,200,{id,title:'消息测试',body:'内容已发送',created_at:new Date().toISOString(),attachments:[],deliveries:[{id:'d1',provider:'我的 Telegram',status:'sent',attempts:1,error:null}]});return;
    }
    if(req.method==='PATCH' && url.pathname==='/v1/admin/site'){
      Object.assign(site,await bodyOf(req));
      json(res,200,site);return;
    }
    json(res,404,{error:'Not found: '+url.pathname});
  }catch(error){json(res,500,{error:error.message});}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const address='http://127.0.0.1:'+server.address().port;
await mkdir(captures,{recursive:true});
let browser;
try{
  browser=await chromium.launch({headless:true});
  const desktop=await browser.newPage({viewport:{width:1440,height:900},deviceScaleFactor:1});
  const errors=[];
  desktop.on('pageerror',error=>errors.push(error.message));
  await desktop.goto(address,{waitUntil:'networkidle'});
  assert.equal(await desktop.locator('#authScreen').isVisible(),true);
  assert.equal(await desktop.locator('#registerTab').isVisible(),true);
  await desktop.screenshot({path:path.join(captures,'login-desktop.png'),fullPage:true});
  await desktop.locator('#loginUsername').fill('administrator');
  await desktop.locator('#loginPassword').fill('example-password');
  await desktop.locator('#loginForm button[type=submit]').click();
  await desktop.locator('#appShell:not([hidden])').waitFor();
  assert.equal(await desktop.locator('#statProviders').textContent(),'0');
  await desktop.screenshot({path:path.join(captures,'dashboard-desktop.png'),fullPage:true});

  await desktop.locator('.nav-item[data-view=providers]').click();
  await desktop.locator('#addProviderButton').click();
  await desktop.locator('#providerDialog[open]').waitFor();
  assert.equal(await desktop.locator('#field-bot_token').isVisible(),true);
  await desktop.locator('#providerKind').selectOption('email_smtp');
  assert.equal(await desktop.locator('#field-host').isVisible(),true);
  assert.equal(await desktop.locator('#providerMode').isDisabled(),true);
  await desktop.locator('#providerKind').selectOption('telegram');
  await desktop.locator('#providerName').fill('我的 Telegram');
  await desktop.locator('#field-bot_token').fill('123456:TEST');
  await desktop.locator('#field-chat_id').fill('12345');
  await desktop.locator('#providerForm button[type=submit]').click();
  await desktop.locator('#providerCards .provider-card').waitFor();
  assert.equal(await desktop.locator('#providerCards .provider-card').count(),1);
  await desktop.screenshot({path:path.join(captures,'providers-desktop.png'),fullPage:true});

  await desktop.locator('.nav-item[data-view=site]').click();
  assert.equal(await desktop.locator('#allowRegistration').isChecked(),true);
  await desktop.locator('#siteName').fill('消息中心');
  await desktop.locator('#siteForm button[type=submit]').click();
  await desktop.waitForFunction(()=>document.querySelector('#sidebarSiteName').textContent==='消息中心');
  assert.equal(site.site_name,'消息中心');
  await desktop.screenshot({path:path.join(captures,'settings-desktop.png'),fullPage:true});

  const registration=await browser.newPage({viewport:{width:1200,height:840}});
  await registration.goto(address,{waitUntil:'networkidle'});
  await registration.locator('#registerTab').click();
  assert.equal(await registration.locator('#registerForm').isVisible(),true);
  await registration.locator('#registerUsername').fill('newmember');
  await registration.locator('#registerPassword').fill('password-of-sufficient-length');
  await registration.locator('#registerConfirm').fill('password-of-sufficient-length');
  await registration.locator('#registerForm button[type=submit]').click();
  await registration.locator('#appShell:not([hidden])').waitFor();
  assert.equal(await registration.locator('.nav-item[data-view=users]').isVisible(),false);
  assert.equal(await registration.locator('#accountPermission').textContent(),'普通用户');

  const mobile=await browser.newPage({viewport:{width:390,height:844},deviceScaleFactor:1});
  await mobile.goto(address,{waitUntil:'networkidle'});
  await mobile.screenshot({path:path.join(captures,'login-mobile.png'),fullPage:true});
  await mobile.locator('#loginUsername').fill('administrator');
  await mobile.locator('#loginPassword').fill('example-password');
  await mobile.locator('#loginForm button[type=submit]').click();
  await mobile.locator('#appShell:not([hidden])').waitFor();
  await mobile.screenshot({path:path.join(captures,'dashboard-mobile.png'),fullPage:true});
  assert.equal(await mobile.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth+1),true,'mobile dashboard has horizontal overflow');
  await mobile.locator('#mobileMenu').click();
  assert.equal(await mobile.locator('#sidebar').evaluate(e=>e.classList.contains('open')),true);
  await mobile.locator('.nav-item[data-view=keys]').click();
  assert.equal(await mobile.locator('#view-keys').isVisible(),true);
  assert.equal(await mobile.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth+1),true,'mobile keys view has horizontal overflow');
  assert.deepEqual(errors,[]);
  console.log('Browser tests passed: desktop login, registration, channel editor, site settings, mobile navigation and layout.');
} finally {
  if(browser)await browser.close();
  server.close();
}
