'use strict';

const $ = (id) => document.getElementById(id);
const state = {
  token: sessionStorage.getItem('mp_session') || '',
  site: {site_name:'MessagePusher',site_description:'一个接口，发送到你需要的每个平台。',announcement:'',allow_registration:false},
  me: null, providers: [], keys: [], events: [], users: [], overview: null,
  catalog: [], adminCatalog: [],
  view: 'overview', currentKey: '', providerId: null, userId: null
};
const pages = {
  overview: ['工作空间概览','了解通知发送情况，快速管理你的工作空间。','WORKSPACE OVERVIEW','概览'],
  send: ['发送消息','选择接收平台，编辑通知并添加附件。','NEW NOTIFICATION','发送消息'],
  providers: ['推送渠道','集中管理各平台账户、接收者授权和消息代理。','DELIVERY PROVIDERS','推送渠道'],
  keys: ['API 密钥','每个应用使用独立的发送凭据。','API ACCESS','API 密钥'],
  history: ['消息记录','查看每条通知在各个平台的发送结果。','MESSAGE ACTIVITY','消息记录'],
  users: ['用户管理','创建账户、分配权限及管理账户状态。','USER MANAGEMENT','用户管理'],
  site: ['站点设置','管理站点的公开信息和用户注册方式。','SITE CONFIGURATION','站点设置'],
  account: ['我的账户','查看账户信息，维护登录密码。','MY ACCOUNT','我的账户']
};
const channels = {
  telegram: { title:'Telegram 机器人',icon:'i-send',help:'在 @BotFather 创建机器人。个人接收者应先打开订阅链接与机器人建立会话。',fields:[
    ['bot_token','Bot Token','password',true],
    ['bot_username','机器人用户名','text',false],
    ['webhook_secret','Webhook 密钥','password',false],
    ['chat_id','Chat ID（可选）','text',false]
  ]},
  wecom_app: { title:'企业微信自建应用',icon:'i-channel',help:'请先在企业微信管理后台创建应用，并设置网页授权可信域名。授权后接收者的 UserID 将自动保存。',fields:[
    ['corp_id','企业 CorpID','text',true],
    ['corp_secret','应用 Secret','password',true],
    ['agent_id','应用 AgentID','number',true],
    ['to_user','接收者 UserID（可选）','text',false]
  ]},
  wecom_webhook: { title:'企业微信群机器人',icon:'i-channel',help:'填写群机器人 Webhook 链接中 key= 后面的内容。',fields:[
    ['webhook_key','群机器人 Webhook Key','password',true]
  ]},
  email_smtp: { title:'SMTP 电子邮件',icon:'i-mail',help:'由 Rust 主程序通过 SMTP 直接发送，支持 STARTTLS、隐式 TLS 和 MIME 附件。',fields:[
    ['host','SMTP 服务器','text',true],
    ['port','服务器端口','number',false],
    ['tls','连接加密方式','select',false,['starttls','implicit','none']],
    ['username','SMTP 用户名','text',false],
    ['password','SMTP 密码 / 授权码','password',false],
    ['from','发件邮箱','text',true],
    ['to','收件邮箱（逗号分隔）','text',true]
  ]},
  email_resend: { title:'Resend 邮件 API',icon:'i-mail',help:'使用 Resend HTTP API 发送邮件，需要经过验证的发件域名。',fields:[
    ['api_key','Resend API Key','password',true],
    ['from','发件邮箱','text',true],
    ['to','收件邮箱（逗号分隔）','text',true]
  ]}
};
const icons = {
  ok:'i-check',loading:'i-clock',file:'i-file'
};
function svg(id) {
  const icon = document.createElementNS('http://www.w3.org/2000/svg','svg');
  icon.setAttribute('class','icon');
  icon.setAttribute('aria-hidden','true');
  const use = document.createElementNS('http://www.w3.org/2000/svg','use');
  use.setAttribute('href','#'+id);
  icon.append(use);
  return icon;
}
function node(tag,cls,text) {
  const item=document.createElement(tag);
  if (cls) item.className=cls;
  if (text !== undefined && text !== null) item.textContent=String(text);
  return item;
}
function setText(id,value){$(id).textContent=String(value ?? '');}
function showToast(message,error=false) {
  const item=node('div',error?'toast error':'toast');
  item.append(svg(error?'i-info':'i-check'),node('span','',message));
  const container=$('toastArea');
  while(container.children.length>=2)container.firstElementChild.remove();
  container.append(item);
  setTimeout(()=>item.remove(),error?5200:3500);
}
function friendlyError(error){
  const text=String(error?.message || error);
  if(text==='Authentication required')return '登录状态已失效，请重新登录。';
  if(text==='Permission denied')return '没有权限执行此操作，或站点已关闭注册。';
  if(text==='username already exists')return '这个用户名已被占用，请使用其他用户名。';
  return text;
}
function deliveryErrorMessage(error){
  const text=String(error||'');
  if(/connection refused/i.test(text))return '连接失败：目标服务器拒绝连接';
  if(/timed out|timeout|deadline exceeded/i.test(text))return '连接超时，请检查网络和服务器状态';
  if(/name or service not known|failed to lookup|dns error/i.test(text))return '无法解析目标服务器地址';
  if(/authentication failed/i.test(text))return '推送平台身份验证失败';
  const http=text.match(/provider returned HTTP (\d{3})/i);
  if(http)return '推送平台返回 HTTP '+http[1]+' 错误';
  return text;
}
async function request(path,{method='GET',body,auth=true,key}={}) {
  const headers = new Headers({'Accept':'application/json'});
  if (key !== undefined) headers.set('Authorization','Bearer '+key);
  else if (auth && state.token) headers.set('Authorization','Bearer '+state.token);
  const options={method,headers,cache:'no-store'};
  if (body !== undefined){headers.set('Content-Type','application/json');options.body=JSON.stringify(body);}
  const response=await fetch(path,options);
  const value=await response.json().catch(()=>({}));
  if(!response.ok)throw new Error(value.error || '请求失败 (HTTP '+response.status+')');
  return value;
}
function setSite(site) {
  state.site=site;
  const name=site.site_name || 'MessagePusher';
  document.title=name+' · 推送管理';
  setText('sidebarSiteName',name);
  setText('authSiteDescription',site.site_description || '将邮件、Telegram、企业微信整合到同一个地方。');
  $('registerTab').hidden=!site.allow_registration;
  $('registrationStatus').hidden=site.allow_registration;
  if(!site.allow_registration && !$('registerForm').hidden)setAuthMode('login');
  for(const id of ['siteAnnouncementAuth','siteAnnouncement']){
    const box=$(id);box.textContent=site.announcement||'';box.hidden=!site.announcement;
  }
  $('siteName').value=name;
  $('siteDescription').value=site.site_description||'';
  $('siteAnnouncementInput').value=site.announcement||'';
  $('allowRegistration').checked=Boolean(site.allow_registration);
}
async function loadSite(){
  const site=await request('/v1/site',{auth:false});
  setSite(site);
}
function setAuthMode(mode){
  if(mode==='register'&&!state.site.allow_registration)return;
  const register=mode==='register';
  $('loginForm').hidden=register;
  $('registerForm').hidden=!register;
  $('loginTab').classList.toggle('active',!register);
  $('registerTab').classList.toggle('active',register);
  $('loginTab').setAttribute('aria-selected',String(!register));
  $('registerTab').setAttribute('aria-selected',String(register));
  setText('authTitle',register?'创建你的账户':'登录你的工作空间');
  setText('authSubtitle',register?'注册后即可管理属于自己的通知与平台。':'管理推送渠道、发送密钥与每一条通知。');
}
function showAuth(){
  $('authScreen').hidden=false;
  $('appShell').hidden=true;
  setAuthMode('login');
  $('loginPassword').value='';
}
function logout(message){
  state.token='';state.me=null;state.currentKey='';
  sessionStorage.removeItem('mp_session');
  $('sendKey').value='';
  showAuth();
  if(message)showToast(message);
}
function showApp(){
  $('authScreen').hidden=true;
  $('appShell').hidden=false;
  const me=state.me;
  const role=me.role==='admin'?'管理员':'普通用户';
  const initials=me.username.slice(0,2).toUpperCase();
  setText('headerAvatar',initials);setText('accountAvatar',initials);
  setText('headerUsername',me.username);setText('headerRole',role);
  setText('accountUsername',me.username);
  setText('accountRole',role);setText('accountPermission',role);
  document.querySelectorAll('.admin-only').forEach(x=>x.hidden=me.role!=='admin');
  const requested=(location.hash||'').replace(/^#/,'');
  const current=(pages[requested]?requested:'overview');
  navigate(me.role==='admin'?current:((current==='users'||current==='site')?'overview':current));
}
async function refreshAll(){
  if(!state.token)return;
  const me=await request('/v1/me');
  state.me=me;
  const tasks=[
    request('/v1/providers'),
    request('/v1/keys'),
    request('/v1/events'),
    loadSite(),
    me.role==='admin'?request('/v1/users'):Promise.resolve([]),
    me.role==='admin'?request('/v1/admin/overview'):Promise.resolve(null),
    request('/v1/provider-catalog'),
    me.role==='admin'?request('/v1/admin/provider-catalog'):Promise.resolve([])
  ];
  const [providers,keys,events,,users,overview,catalog,adminCatalog]=await Promise.all(tasks);
  state.providers=providers;state.keys=keys;state.events=events;
  state.users=users;state.overview=overview;
  state.catalog=catalog;state.adminCatalog=adminCatalog;
  showApp();
  renderAll();
}
async function safeRefresh(){
  try{await refreshAll();return true;}
  catch(error){
    if(/Authentication required|Permission denied/.test(String(error.message))){logout('请重新登录。');}
    else showToast(friendlyError(error),true);
    return false;
  }
}
function navigate(view){
  if(!pages[view] || ((view==='site'||view==='users') && state.me?.role!=='admin'))return;
  state.view=view;
  for(const element of document.querySelectorAll('.view')){
    element.hidden=element.id!=='view-'+view;
  }
  for(const item of document.querySelectorAll('.nav-item')){
    const active=item.dataset.view===view;
    item.classList.toggle('active',active);
    if(active)item.setAttribute('aria-current','page');
    else item.removeAttribute('aria-current');
  }
  const [title,desc,eyebrow,short]=pages[view];
  setText('pageTitle',title);setText('pageDescription',desc);
  setText('pageEyebrow',eyebrow);setText('headerPath','工作空间 / '+short);
  history.replaceState(null,'','#'+view);
  closeSidebar();
  window.scrollTo({top:0,behavior:'smooth'});
}
function openSidebar(){
  $('sidebar').classList.add('open');
  $('sidebarShade').hidden=false;
  $('mobileMenu').setAttribute('aria-expanded','true');
}
function closeSidebar(){
  $('sidebar').classList.remove('open');
  $('sidebarShade').hidden=true;
  $('mobileMenu').setAttribute('aria-expanded','false');
}
function makeButton(text,handler,style='ghost',iconName){
  const button=node('button','btn small '+style);
  button.type='button';
  if(iconName)button.append(svg(iconName));
  button.append(node('span','',text));
  button.addEventListener('click',async()=>{
    button.disabled=true;
    try{await handler();}
    catch(error){showToast(friendlyError(error),true);}
    finally{button.disabled=false;}
  });
  return button;
}
function empty(title,description,iconName='i-info',button){
  const container=node('div','empty');
  const symbol=node('div','empty-icon');symbol.append(svg(iconName));
  container.append(symbol,node('strong','',title),node('p','',description));
  if(button)container.append(button);
  return container;
}
function formatDate(timestamp){
  if(!timestamp)return '—';
  const date=new Date(timestamp);
  return Number.isNaN(date.getTime())?'—':date.toLocaleString('zh-CN',{year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit'});
}
function status(event){
  if(event.failed>0)return {name:'部分失败',cls:'error'};
  if(event.total>0&&event.sent===event.total)return {name:'全部成功',cls:''};
  return {name:'发送中',cls:'pending'};
}
function tag(title,cls=''){return node('span','tag '+cls,title);}
function renderOverview(){
  setText('statProviders',state.providers.length);
  setText('statKeys',state.keys.length);
  setText('statEvents',state.events.length);
  setText('statPending',state.events.reduce((sum,e)=>sum+Math.max(0,e.total-e.sent-e.failed),0));
  const parent=$('overviewEvents');parent.replaceChildren();
  if(!state.events.length){
    parent.append(empty('暂无消息','发送第一条消息后，最近动态会显示在这里。','i-history'));
    return;
  }
  for(const e of state.events.slice(0,4)){
    const row=node('div','activity-row');
    const mark=node('div','activity-icon');mark.append(svg('i-bell'));
    const copy=node('div','activity-copy');
    copy.append(node('strong','',e.title||'（无标题）'),node('small','',status(e).name+' · 已发送 '+e.sent+'/'+e.total));
    row.append(mark,copy,node('time','activity-time',formatDate(e.created_at)));
    parent.append(row);
  }
}
function providerTitle(kind){return channels[kind]?.title||kind;}
function renderProviders(){
  const parent=$('providerCards');parent.replaceChildren();
  if(!state.providers.length){
    const action=state.catalog.length?makeButton('添加第一个渠道',()=>openProviderDialog(), 'primary','i-plus'):null;
    const box=empty('还没有推送渠道',state.catalog.length?
      '添加一个已开放的推送渠道，即可开始发送。':'本站目前尚未开放任何 Provider 类型。','i-channel',action);
    box.style.gridColumn='1/-1';parent.append(box);return;
  }
  for(const p of state.providers){
    const card=node('article','provider-card');
    const header=node('div','provider-header');
    const symbol=node('div','provider-logo '+p.kind);symbol.append(svg(channels[p.kind]?.icon||'i-channel'));
    const info=node('div','provider-info');info.append(node('h3','',p.name),node('small','',providerTitle(p.kind)));
    const usable=p.enabled && p.available!==false;
    header.append(symbol,info,tag(p.available===false?'站点已暂停':(p.enabled?'已启用':'已暂停'),usable?'':'paused'));
    const meta=node('div','provider-meta');
    meta.append(node('span','',p.delivery==='proxy'?'经消息代理发送':'主程序直接发送'));
    if(!p.subscribed)meta.append(tag('等待接收者绑定','pending'));
    if(p.available===false)meta.append(node('span','','管理员暂停了该 Provider 类型，现有配置已保留'));
    const actions=node('div','provider-actions');
    actions.append(makeButton('编辑',()=>openProviderDialog(p.id),'ghost','i-edit'));
    actions.append(makeButton(p.enabled?'暂停':'启用',async()=>{
      await request('/v1/providers/'+p.id,{method:'PATCH',body:{enabled:!p.enabled}});
      await refreshAll();showToast('渠道状态已更新。');
    },'ghost'));
    if(p.kind==='telegram'){
      actions.append(makeButton('设置 Webhook',async()=>{
        await request('/v1/providers/'+p.id+'/telegram/webhook',{method:'POST'});
        showToast('Telegram Webhook 已设置。');
      },'secondary'));
      actions.append(makeButton('订阅链接',async()=>{
        const result=await request('/v1/providers/'+p.id+'/telegram/subscribe',{method:'POST'});
        showAuthorization(result.url);
      },'ghost'));
    }
    if(p.kind==='wecom_app'){
      actions.append(makeButton('授权链接',async()=>{
        const result=await request('/v1/providers/'+p.id+'/wecom/subscribe',{method:'POST'});
        showAuthorization(result.url);
      },'secondary'));
    }
    actions.append(makeButton('删除',async()=>{
      if(!window.confirm('确定删除渠道「'+p.name+'」吗？发送记录仍然保留。'))return;
      await request('/v1/providers/'+p.id,{method:'DELETE'});
      await refreshAll();showToast('推送渠道已删除。');
    },'danger','i-trash'));
    card.append(header,meta,actions);parent.append(card);
  }
}
function renderKeys(){
  const parent=$('keysList');parent.replaceChildren();
  if(!state.keys.length){
    parent.append(empty('没有 API 密钥','先创建密钥，再将它填到需要发送通知的应用中。','i-key'));
    return;
  }
  const list=node('div','data-list');
  for(const k of state.keys){
    const row=node('div','data-row');
    const left=node('div');
    const title=node('div','data-title');
    const mark=node('span','small-icon');mark.append(svg('i-key'));
    title.append(mark,node('span','',k.name));
    left.append(title,node('div','data-subtitle',k.prefix+'… · 创建于 '+formatDate(k.created_at)));
    const actions=node('div','data-actions');
    actions.append(makeButton('撤销密钥',async()=>{
      if(!confirm('确定撤销「'+k.name+'」吗？使用这把密钥的应用将无法继续发送消息。'))return;
      await request('/v1/keys/'+k.id,{method:'DELETE'});
      await refreshAll();showToast('密钥已撤销。');
    },'danger','i-trash'));
    row.append(left,actions);list.append(row);
  }
  parent.append(list);
}
function renderTargets(){
  const target=$('targetProviders');target.replaceChildren();
  const choices=state.providers.filter(p=>p.enabled && p.available!==false);
  if(!choices.length){target.append(node('p','hint','请先添加并启用至少一个推送渠道。'));return;}
  for(const p of choices){
    const label=node('label','choice');
    const checkbox=document.createElement('input');checkbox.type='checkbox';checkbox.value=p.id;checkbox.name='targetProvider';
    label.append(checkbox,node('span','',p.name));
    target.append(label);
  }
}
function renderHistory(){
  const parent=$('historyList');parent.replaceChildren();
  if(!state.events.length){
    parent.append(empty('暂无发送记录','发送第一条消息后，可以在这里查看每个平台的结果。','i-history'));
    $('eventDetails').hidden=true;return;
  }
  const list=node('div','data-list');
  for(const e of state.events){
    const row=node('div','data-row');
    const left=node('div');
    const title=node('div','data-title');title.append(node('span','',e.title||'（无标题）'),tag(status(e).name,status(e).cls));
    left.append(title,node('div','data-subtitle',formatDate(e.created_at)+' · 成功 '+e.sent+'/'+e.total+(e.failed?' · 失败 '+e.failed:'')));
    const actions=node('div','data-actions');
    actions.append(makeButton('查看详情',()=>showEvent(e.id),'ghost','i-chevron'));
    row.append(left,actions);list.append(row);
  }
  parent.append(list);
}
async function showEvent(id){
  const event=await request('/v1/events/'+id);
  const container=$('eventDetails');container.replaceChildren();container.hidden=false;
  container.append(node('h3','',event.title||'（无标题）'),node('p','muted',formatDate(event.created_at)));
  const body=node('div','event-body',event.body||'（无正文）');container.append(body);
  if(event.attachments?.length){
    container.append(node('h3','','附件'));
    for(const file of event.attachments)container.append(node('div','data-subtitle',file.filename+' · '+file.content_type));
  }
  container.append(node('h3','','各渠道发送情况'));
  for(const d of event.deliveries){
    const row=node('div','delivery-row');
    const copy=node('div');
    const hint=node('small','',(d.error?deliveryErrorMessage(d.error)+' · ':'')+'尝试 '+d.attempts+' 次');
    if(d.error)hint.title=d.error;
    copy.append(node('strong','',d.provider),hint);
    const statusMap={sent:['已发送',''],failed:['发送失败','error'],working:['发送中','pending'],pending:['等待重试','pending']};
    row.append(copy,tag(...(statusMap[d.status]||[d.status,'pending'])));
    container.append(row);
  }
  container.scrollIntoView({behavior:'smooth',block:'nearest'});
}
function renderUsers(){
  const parent=$('usersList');parent.replaceChildren();
  if(!state.users.length){parent.append(empty('暂无用户','创建一个普通用户账户开始使用。','i-users'));return;}
  const list=node('div','data-list');
  for(const u of state.users){
    const row=node('div','data-row');
    const left=node('div');
    const title=node('div','data-title');
    const badge=node('span','small-icon');badge.append(svg('i-user'));
    title.append(badge,node('span','',u.username),tag(u.role==='admin'?'管理员':'普通用户',u.role==='admin'?'':'paused'));
    left.append(title,node('div','data-subtitle',(u.active?'账户正常':'账户已停用')+' · 创建于 '+formatDate(u.created_at)));
    const actions=node('div','data-actions');
    if(u.id!==state.me.id){
      actions.append(makeButton('编辑',()=>openUserDialog(u.id),'ghost','i-edit'));
      actions.append(makeButton(u.active?'停用':'启用',async()=>{
        await request('/v1/users/'+u.id,{method:'PATCH',body:{active:!u.active}});
        await refreshAll();showToast('账户状态已更新。');
      },'ghost'));
      actions.append(makeButton('删除',async()=>{
        if(!confirm('确定删除用户「'+u.username+'」及其密钥、平台和发送记录吗？此操作无法撤销。'))return;
        await request('/v1/users/'+u.id,{method:'DELETE'});
        await refreshAll();showToast('用户已删除。');
      },'danger','i-trash'));
    }
    row.append(left,actions);list.append(row);
  }
  parent.append(list);
}
function renderProviderTypeOptions(selectedKind){
  const select=$('providerKind');
  const allowed=state.catalog;
  const current=selectedKind||select.value;
  select.replaceChildren();
  for(const entry of allowed){
    const option=node('option','',entry.label);
    option.value=entry.kind;
    select.append(option);
  }
  if(selectedKind&&!allowed.some(entry=>entry.kind===selectedKind)){
    const option=node('option','',providerTitle(selectedKind)+'（站点已暂停）');
    option.value=selectedKind;
    select.append(option);
  }
  select.value=Array.from(select.options).some(option=>option.value===current)?current:(select.options[0]?.value||'');
  $('addProviderButton').disabled=allowed.length===0;
}
function renderCatalog(){
  const parent=$('providerCatalogList');
  parent.replaceChildren();
  if(state.me?.role!=='admin')return;
  for(const entry of state.adminCatalog){
    const row=node('div','catalog-row');
    const mark=node('span','provider-logo catalog-provider-logo '+entry.kind);
    mark.append(svg(channels[entry.kind]?.icon||'i-channel'));
    const info=node('div','catalog-info');
    info.append(node('strong','',entry.label),node('p','',entry.description));
    const status=tag(entry.enabled?'向用户开放':'已暂停',entry.enabled?'':'paused');
    const label=node('label','switch');
    const control=document.createElement('input');
    control.type='checkbox';control.checked=entry.enabled;
    control.setAttribute('aria-label','向用户开放 '+entry.label);
    const track=node('span','switch-track');
    control.addEventListener('change',async()=>{
      control.disabled=true;
      try{
        await request('/v1/admin/provider-catalog/'+encodeURIComponent(entry.kind),{
          method:'PATCH',body:{enabled:control.checked}
        });
        await refreshAll();
        showToast(entry.label+(control.checked?' 已向用户开放。':' 已暂停对外提供。'));
      }catch(error){
        control.checked=entry.enabled;
        showToast(friendlyError(error),true);
      }finally{control.disabled=false;}
    });
    label.append(control,track);
    row.append(mark,info,status,label);
    parent.append(row);
  }
}
function renderAll(){
  renderOverview();renderProviders();renderKeys();renderTargets();renderHistory();renderUsers();
  renderCatalog();renderProviderTypeOptions(state.providerDetails?.kind);
}
function showDialog(id){$(id).showModal();}
function closeDialog(id){$(id).close();}
function renderProviderFields(kind,details){
  const parent=$('providerFields');parent.replaceChildren();
  const schema=channels[kind];
  if(!schema)return;
  for(const [key,labelText,type,required,options] of schema.fields){
    const field=node('div','field '+(key==='to'||key==='from'||key==='host'||key==='bot_token'||key==='webhook_key'?'wide-field':''));
    const label=node('label','',labelText);label.htmlFor='field-'+key;
    let control;
    if(type==='select'){
      control=document.createElement('select');
      for(const value of options){const option=node('option','',value==='implicit'?'隐式 TLS（常见于 465）':value==='starttls'?'STARTTLS（常见于 587）':'不加密',);option.value=value;control.append(option);}
    }else{
      control=document.createElement('input');control.type=type;
    }
    control.id='field-'+key;control.dataset.setting=key;
    control.dataset.secret=type==='password'?'true':'false';
    control.required=Boolean(required&&!details);
    const value=details?.settings?.[key];
    if(value!==undefined&&value!==null){control.value=Array.isArray(value)?value.join(', '):String(value);}
    else if(type==='select'){control.value='starttls';}
    if(type==='password'){
      control.autocomplete='new-password';
      control.placeholder=details?.stored_secrets?.includes(key)?'已保存，留空则保留原值':'请输入 '+labelText;
    }else if(key==='to')control.placeholder='name@example.com, other@example.com';
    else if(key==='chat_id'||key==='to_user')control.placeholder='留空可通过授权链接绑定';
    field.append(label,control);parent.append(field);
  }
  setText('providerHelp',schema.help);
}
function updateProviderMode(){
  const proxy=$('providerMode').value==='proxy';
  $('providerRelayFields').hidden=!proxy;
  $('providerRelayUrl').required=proxy;
  $('providerRelaySecret').required=proxy && !(state.providerDetails?.delivery?.secret_saved);
}
function chooseProviderKind(){
  const kind=$('providerKind').value;
  const smtp=kind==='email_smtp';
  $('providerMode').disabled=smtp;
  if(smtp)$('providerMode').value='direct';
  renderProviderFields(kind,state.providerDetails);
  updateProviderMode();
}
async function openProviderDialog(id){
  state.providerId=id||null;
  state.providerDetails=id?await request('/v1/providers/'+id):null;
  const details=state.providerDetails;
  if(!details && !state.catalog.length)throw new Error('站点管理员尚未开放任何 Provider 类型。');
  setText('providerDialogTitle',id?'编辑推送渠道':'添加推送渠道');
  $('providerName').value=details?.name||'';
  renderProviderTypeOptions(details?.kind);
  if(details)$('providerKind').value=details.kind;
  $('providerKind').disabled=Boolean(id);
  $('providerEnabled').checked=details?.enabled??true;
  $('providerMode').value=details?.delivery?.mode||'direct';
  $('providerRelayUrl').value=details?.delivery?.url||'';
  $('providerRelaySecret').value='';
  chooseProviderKind();
  showDialog('providerDialog');
}
function providerPayload(){
  const kind=$('providerKind').value;
  const fields={};
  for(const input of $('providerFields').querySelectorAll('[data-setting]')){
    const key=input.dataset.setting;
    const value=String(input.value).trim();
    if(input.dataset.secret==='true'&&!value&&state.providerId)continue;
    if(!value){
      if(state.providerId&&input.type!=='password')fields[key]=null;
      continue;
    }
    if(key==='port'||key==='agent_id'){
      const number=Number(value);
      if(!Number.isSafeInteger(number)||number<=0)throw new Error('端口与应用 ID 必须是正整数。');
      fields[key]=number;
    }else if(key==='to'){
      const recipients=value.split(',').map(x=>x.trim()).filter(Boolean);
      if(!recipients.length)throw new Error('请填写至少一个收件邮箱。');
      fields[key]=recipients;
    }else{fields[key]=value;}
  }
  const mode=$('providerMode').value;
  const delivery=mode==='direct'?{mode:'direct'}:{
    mode:'proxy',url:$('providerRelayUrl').value.trim(),
    secret:$('providerRelaySecret').value
  };
  if(mode==='proxy'&&!delivery.secret&&state.providerId)delete delivery.secret;
  return {name:$('providerName').value.trim(),kind,enabled:$('providerEnabled').checked,settings:fields,delivery};
}
function showAuthorization(url){
  const link=new URL(url);
  if(link.protocol!=='https:'||!['t.me','open.weixin.qq.com'].includes(link.hostname)){
    throw new Error('授权地址无效');
  }
  $('linkValue').value=url;
  showDialog('linkDialog');
}
function openUserDialog(id){
  state.userId=id||null;
  const user=state.users.find(x=>x.id===id);
  setText('userDialogTitle',user?'编辑用户':'添加用户');
  $('manageUsername').value=user?.username||'';
  $('manageUsername').disabled=Boolean(user);
  $('manageRole').value=user?.role||'user';
  $('manageRole').disabled=Boolean(user&&user.id===state.me.id);
  $('managePassword').value='';
  $('managePassword').required=!user;
  $('manageActiveRow').hidden=!user;
  $('manageActive').checked=user?.active??true;
  $('manageActive').disabled=Boolean(user&&user.id===state.me.id);
  setText('managePasswordHint',user?'留空则保留原密码，填写新密码可重置账户密码。':'创建账户时必须填写至少 12 个字符的密码。');
  showDialog('userDialog');
}
function showFileSelection(){
  const files=Array.from($('sendFiles').files);
  const target=$('fileSelection');target.replaceChildren();
  for(const file of files)target.append(node('span','file-pill',file.name+' · '+(file.size/1024).toFixed(1)+' KB'));
}
async function fileContent(file){
  const bytes=new Uint8Array(await file.arrayBuffer());
  let binary='';
  for(let i=0;i<bytes.length;i+=8192)binary+=String.fromCharCode(...bytes.subarray(i,i+8192));
  return {filename:file.name,content_type:file.type||'application/octet-stream',data_base64:btoa(binary)};
}
async function copyValue(value){
  try{await navigator.clipboard.writeText(value);showToast('已复制到剪贴板。');}
  catch{showToast('无法自动复制，请选中文本后手动复制。',true);}
}
function handleSubmit(id,handler){
  $(id).addEventListener('submit',async(event)=>{
    event.preventDefault();
    const button=event.submitter;
    if(button)button.disabled=true;
    try{await handler(event);}
    catch(error){showToast(friendlyError(error),true);}
    finally{if(button)button.disabled=false;}
  });
}

document.querySelectorAll('[data-view]').forEach(element=>{
  element.addEventListener('click',()=>{
    if(!state.me)return;
    navigate(element.dataset.view);
  });
});
document.querySelectorAll('[data-close]').forEach(button=>{
  button.addEventListener('click',()=>closeDialog(button.dataset.close));
});
$('mobileMenu').addEventListener('click',()=>{
  if($('sidebar').classList.contains('open'))closeSidebar();else openSidebar();
});
$('sidebarShade').addEventListener('click',closeSidebar);
$('logoutButton').addEventListener('click',()=>logout('已安全退出账户。'));
$('loginTab').addEventListener('click',()=>setAuthMode('login'));
$('registerTab').addEventListener('click',()=>setAuthMode('register'));
$('refreshButton').addEventListener('click',async()=>{if(await safeRefresh())showToast('数据已更新。');});
$('refreshHistoryButton').addEventListener('click',safeRefresh);
$('addProviderButton').addEventListener('click',()=>openProviderDialog().catch(e=>showToast(friendlyError(e),true)));
$('providerKind').addEventListener('change',()=>{state.providerDetails=null;chooseProviderKind();});
$('providerMode').addEventListener('change',updateProviderMode);
$('addKeyButton').addEventListener('click',()=>{$('keyForm').reset();showDialog('keyDialog');});
$('addUserButton').addEventListener('click',()=>openUserDialog());
$('sendFiles').addEventListener('change',showFileSelection);
document.querySelectorAll('[name="sendMode"]').forEach(element=>element.addEventListener('change',()=>{
  $('targetProviders').hidden=document.querySelector('[name="sendMode"]:checked').value!=='select';
}));
$('copyKeyButton').addEventListener('click',()=>copyValue($('newKey').value));
$('copyLinkButton').addEventListener('click',()=>copyValue($('linkValue').value));
$('openLinkButton').addEventListener('click',()=>window.open($('linkValue').value,'_blank','noopener,noreferrer'));

handleSubmit('loginForm',async()=>{
  const result=await request('/v1/login',{auth:false,method:'POST',body:{
    username:$('loginUsername').value,password:$('loginPassword').value
  }});
  state.token=result.token;sessionStorage.setItem('mp_session',state.token);
  $('loginPassword').value='';
  await refreshAll();
  showToast('欢迎回来。');
});
handleSubmit('registerForm',async()=>{
  if(!state.site.allow_registration)throw new Error('本站尚未开放用户注册。');
  if($('registerPassword').value!==$('registerConfirm').value)throw new Error('两次输入的密码不一致。');
  const result=await request('/v1/register',{auth:false,method:'POST',body:{
    username:$('registerUsername').value,password:$('registerPassword').value
  }});
  state.token=result.token;sessionStorage.setItem('mp_session',state.token);
  $('registerPassword').value='';$('registerConfirm').value='';
  await refreshAll();
  showToast('账户创建成功，欢迎使用！');
});
handleSubmit('providerForm',async()=>{
  const payload=providerPayload();
  if(state.providerId){
    const {kind,...change}=payload;
    await request('/v1/providers/'+state.providerId,{method:'PATCH',body:change});
  }else{await request('/v1/providers',{method:'POST',body:payload});}
  closeDialog('providerDialog');
  state.providerId=null;state.providerDetails=null;
  await refreshAll();showToast('推送渠道已保存。');
});
handleSubmit('keyForm',async()=>{
  const result=await request('/v1/keys',{method:'POST',body:{
    name:$('keyName').value,format:$('keyFormat').value
  }});
  state.currentKey=result.key;
  $('newKey').value=result.key;
  $('keyReveal').hidden=false;
  $('sendKey').value=result.key;
  closeDialog('keyDialog');
  await refreshAll();navigate('keys');
  $('keyReveal').scrollIntoView({behavior:'smooth',block:'center'});
  showToast('已生成新密钥，请立即复制保存。');
});
handleSubmit('sendForm',async()=>{
  const files=Array.from($('sendFiles').files);
  if(files.length>5||files.reduce((sum,file)=>sum+file.size,0)>2*1024*1024)throw new Error('最多上传 5 个文件，总大小不得超过 2 MiB。');
  const attachments=await Promise.all(files.map(fileContent));
  const payload={title:$('sendTitle').value,body:$('sendBody').value,attachments};
  const selected=document.querySelector('[name="sendMode"]:checked').value==='select';
  if(selected){
    payload.provider_ids=Array.from(document.querySelectorAll('[name="targetProvider"]:checked')).map(x=>x.value);
    if(!payload.provider_ids.length)throw new Error('请至少选择一个推送渠道。');
  }
  const sendKey=$('sendKey').value.trim();
  const result=await request('/v1/send',{auth:false,method:'POST',key:sendKey,body:payload});
  $('sendFiles').value='';showFileSelection();
  await refreshAll();navigate('history');
  await showEvent(result.id);
  showToast('消息已加入发送队列。');
});
handleSubmit('userForm',async()=>{
  const user=state.users.find(x=>x.id===state.userId);
  if(user){
    const change={role:$('manageRole').value,active:$('manageActive').checked};
    if($('managePassword').value)change.password=$('managePassword').value;
    await request('/v1/users/'+user.id,{method:'PATCH',body:change});
  }else{
    await request('/v1/users',{method:'POST',body:{
      username:$('manageUsername').value,role:$('manageRole').value,password:$('managePassword').value
    }});
  }
  closeDialog('userDialog');
  await refreshAll();showToast('用户信息已保存。');
});
handleSubmit('siteForm',async()=>{
  const newSite=await request('/v1/admin/site',{method:'PATCH',body:{
    site_name:$('siteName').value,
    site_description:$('siteDescription').value,
    announcement:$('siteAnnouncementInput').value,
    allow_registration:$('allowRegistration').checked
  }});
  setSite(newSite);
  showToast('站点设置已保存。');
});
handleSubmit('passwordForm',async()=>{
  if($('newPassword').value!==$('confirmPassword').value)throw new Error('两次输入的新密码不一致。');
  await request('/v1/me/password',{method:'POST',body:{
    current_password:$('currentPassword').value,
    new_password:$('newPassword').value
  }});
  $('passwordForm').reset();
  logout('密码已更新，请使用新密码重新登录。');
});

(async()=>{
  try{await loadSite();}
  catch(error){showToast('无法读取站点设置：'+friendlyError(error),true);}
  if(state.token)await safeRefresh();
  else showAuth();
})();
