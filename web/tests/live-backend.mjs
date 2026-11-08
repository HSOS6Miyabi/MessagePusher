import { chromium } from 'playwright';
import { mkdir } from 'node:fs/promises';
import assert from 'node:assert/strict';
import path from 'node:path';

const base = process.env.MP_LIVE_URL || 'http://127.0.0.1:8080';
const username = process.env.ADMIN_USERNAME || 'administrator';
const password = process.env.ADMIN_PASSWORD;
if (!password) throw new Error('ADMIN_PASSWORD is required for integration tests');
const memberName = 'live_browser_member';
const memberPassword = 'browser-integration-password-2026';
const updatedPassword = 'browser-integration-updated-2026';
const output = path.resolve('web/screenshots/live');
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ headless: true });
const errors = [];
let checked = 0;
const ensure = (condition, description) => { assert.ok(condition, description); checked++; };
const screenshot = async (page, filename) => {
  await page.evaluate(() => document.getElementById('toastArea')?.replaceChildren());
  await page.screenshot({ path:path.join(output, filename + '.png'), fullPage:true, animations:'disabled' });
};
const observe = page => {
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', e => { if (e.type() === 'error') errors.push(e.text()); });
};
const login = async (page, user, pass) => {
  await page.goto(base, { waitUntil: 'networkidle' });
  await page.locator('#loginUsername').fill(user);
  await page.locator('#loginPassword').fill(pass);
  await page.locator('#loginForm button[type=submit]').click();
  await page.locator('#appShell').waitFor({ state:'visible' });
};
const view = async (page, name) => {
  if (await page.locator('#mobileMenu').isVisible()) {
    await page.locator('#mobileMenu').click();
  }
  await page.locator('.nav-item[data-view=' + name + ']').click();
  await page.locator('#view-' + name).waitFor({ state:'visible' });
};
const admin = await browser.newPage({ viewport:{width:1440,height:900} });
const member = await browser.newPage({ viewport:{width:390,height:844} });
observe(admin); observe(member);
try {
  await login(admin, username, password);
  ensure(await admin.locator('.nav-item[data-view=users]').isVisible(), 'Administrator navigation');
  await view(admin, 'users');
  await admin.locator('#addUserButton').click();
  await admin.locator('#manageUsername').fill(memberName);
  await admin.locator('#managePassword').fill(memberPassword);
  await admin.locator('#userDialog button[type=submit]').click();
  await admin.locator('#userDialog').waitFor({ state:'hidden' });
  ensure((await admin.locator('#usersList').textContent()).includes(memberName), 'User created in PostgreSQL');
  await screenshot(admin, '01-live-user-management');

  await login(member, memberName, memberPassword);
  ensure(await member.locator('#accountPermission').textContent() === '普通用户', 'Normal user account');
  ensure(await member.locator('.nav-item[data-view=site]').isHidden(), 'No site administration permission');
  await view(member, 'providers');
  await member.locator('#addProviderButton').click();
  await member.locator('#providerKind').selectOption('email_smtp');
  await member.locator('#providerName').fill('独立测试邮箱');
  await member.locator('#field-host').fill('127.0.0.1');
  await member.locator('#field-port').fill('2525');
  await member.locator('#field-tls').selectOption('none');
  await member.locator('#field-from').fill('sender@example.com');
  await member.locator('#field-to').fill('receiver@example.com');
  await member.locator('#providerDialog button[type=submit]').click();
  await member.locator('#providerDialog').waitFor({ state:'hidden' });
  await member.locator('#providerCards .provider-card').waitFor({ state:'visible' });
  ensure((await member.locator('#providerCards').textContent()).includes('独立测试邮箱'), 'Personal channel saved');
  await screenshot(member, '02-live-personal-channel');

  await view(member, 'keys');
  await member.locator('#addKeyButton').click();
  await member.locator('#keyName').fill('浏览器发送测试');
  await member.locator('#keyDialog button[type=submit]').click();
  await member.locator('#keyDialog').waitFor({ state:'hidden' });
  ensure((await member.locator('#newKey').inputValue()).startsWith('mpk_'), 'Sending key created');
  await view(member, 'send');
  await member.locator('#sendTitle').fill('真实浏览器消息测试');
  await member.locator('#sendBody').fill('检查前端与 PostgreSQL 队列的实际交互。');
  await member.locator('#sendFiles').setInputFiles({
    name:'live-browser.txt', mimeType:'text/plain',
    buffer:Buffer.from('MessagePusher live browser test')
  });
  await member.locator('#sendForm button[type=submit]').click();
  await member.locator('#view-history').waitFor({ state:'visible' });
  await member.locator('#eventDetails').waitFor({ state:'visible' });
  ensure((await member.locator('#historyList').textContent()).includes('真实浏览器消息测试'), 'Queued message visible');
  ensure((await member.locator('#eventDetails').textContent()).includes('live-browser.txt'), 'Attachment metadata visible');
  await screenshot(member, '03-live-message-history');

  await view(member, 'account');
  await member.locator('#currentPassword').fill(memberPassword);
  await member.locator('#newPassword').fill(updatedPassword);
  await member.locator('#confirmPassword').fill(updatedPassword);
  await member.locator('#passwordForm button[type=submit]').click();
  await member.locator('#authScreen').waitFor({ state:'visible' });
  ensure(await member.locator('#appShell').isHidden(), 'Password change logs out');
  await login(member, memberName, updatedPassword);
  ensure(await member.locator('#accountPermission').textContent() === '普通用户', 'New password works');

  await view(admin, 'users');
  await admin.evaluate(() => { window.confirm = () => true; });
  const row = admin.locator('#usersList .data-row').filter({ hasText:memberName });
  await row.locator('button:has-text("删除")').click();
  await row.waitFor({ state:'detached' });
  ensure(!(await admin.locator('#usersList').textContent()).includes(memberName), 'Temporary user deleted');
  ensure(errors.length === 0, 'No browser runtime or console errors');
  console.log('REAL BACKEND BROWSER: ' + JSON.stringify({ checks:checked, screenshots:3, runtime_errors:errors.length }));
} finally {
  await browser.close();
}
