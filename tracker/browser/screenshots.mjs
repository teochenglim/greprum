// Screenshots every provisioned dashboard on seeded data (AC0.10.5).
//   GRAFANA=http://127.0.0.1:3000 SITE=demo node tracker/browser/screenshots.mjs
import { chromium } from 'playwright-core';
import { existsSync, mkdirSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const GRAFANA = process.env.GRAFANA || 'http://127.0.0.1:3000';
const SITE = process.env.SITE || 'demo';
const CHROME = [process.env.CHROME_PATH, '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/usr/bin/google-chrome']
  .find((p) => p && existsSync(p));
if (!CHROME) throw new Error('no Chrome found; set CHROME_PATH');

const out = join(root, 'images');
mkdirSync(out, { recursive: true });
const uids = readdirSync(join(root, 'ui', 'grafana', 'dashboards')).filter((f) => f.endsWith('.json')).map((f) => f.replace('.json', ''));

const extra = {
  'greprum-funnels': '&var-step1=%24session_start&var-step2=%24pageview&var-step3=signup&var-step4=%24pageleave&var-window_seconds=3600',
  'greprum-retention': '&var-tz_offset_ms=28800000',
};

// A rich session for the detail screenshot: the one with the most events (needs GREPTIME_HTTP).
let richSession = '';
if (process.env.GREPTIME_HTTP) {
  try {
    const r = await fetch(`${process.env.GREPTIME_HTTP}/v1/sql`, {
      method: 'POST',
      headers: { 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({ sql: `SELECT session_id FROM events WHERE site_id = '${SITE}' AND event_name = '$rage_click' GROUP BY session_id ORDER BY count(*) DESC LIMIT 1` }),
    });
    richSession = (await r.json()).output?.[0]?.records?.rows?.[0]?.[0] || '';
  } catch {}
}

const browser = await chromium.launch({ executablePath: CHROME, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1500, height: 1000 } });
const results = [];
for (const uid of uids) {
  const page = await ctx.newPage();
  const url = `${GRAFANA}/d/${uid}?orgId=1&var-site=${SITE}&from=now-7d&to=now&kiosk${uid === 'greprum-session-detail' && richSession ? `&var-session_id=${encodeURIComponent(richSession)}` : extra[uid] || ''}`;
  await page.goto(url, { waitUntil: 'load', timeout: 60000 });
  await page.waitForTimeout(7000);
  const errors = await page.locator('[data-testid="data-testid Panel status error"], [aria-label="Panel status"]').count();
  await page.screenshot({ path: join(out, `grafana_${uid.replace('greprum-', '').replace(/-/g, '_')}.png`), fullPage: true });
  results.push({ uid, errors });
  await page.close();
}
await browser.close();
console.log(JSON.stringify(results));
