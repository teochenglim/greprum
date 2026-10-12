// Real-browser budget (AC0.10.3): tracker init and click handling on the main thread.
// Uses the system Chrome (or CHROME_PATH); skips cleanly when no browser is available.
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const CHROME = [process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable', '/usr/bin/chromium'].find((p) => p && existsSync(p));
const skip = !CHROME || !existsSync(join(here, '..', 'dist', 't.js'));

describe('AC0.10.3 real browser main-thread budget', { skip: skip && 'no Chrome or tracker not built' }, () => {
  let server, port, browser, bodies = [];

  before(async () => {
    const { chromium } = await import('playwright-core');
    server = http.createServer((req, res) => {
      if (req.url === '/' || req.url.startsWith('/?')) {
        res.setHeader('content-type', 'text/html');
        return res.end(readFileSync(join(here, 'demo', 'index.html')));
      }
      if (req.url === '/t.js') {
        res.setHeader('content-type', 'application/javascript');
        return res.end(readFileSync(join(here, '..', 'dist', 't.js')));
      }
      if (req.method === 'POST' && req.url.startsWith('/collect')) {
        let b = '';
        req.on('data', (c) => (b += c));
        req.on('end', () => { try { bodies.push(JSON.parse(b)); } catch {} res.statusCode = 204; res.end(); });
        return;
      }
      res.statusCode = 404; res.end();
    });
    await new Promise((r) => server.listen(0, '127.0.0.1', r));
    port = server.address().port;
    browser = await chromium.launch({ executablePath: CHROME, headless: true });
  });

  after(async () => { await browser?.close(); server?.close(); });

  async function open() {
    const ctx = await browser.newContext({ userAgent: 'Mozilla/5.0 (Macintosh) AppleWebKit/605 Safari/605' });
    const page = await ctx.newPage();
    await page.addInitScript(() => {
      window.__long = [];
      new PerformanceObserver((l) => l.getEntries().forEach((e) => window.__long.push(e.duration))).observe({ type: 'longtask', buffered: true });
    });
    await page.goto(`http://127.0.0.1:${port}/`, { waitUntil: 'load' });
    return { ctx, page };
  }

  it('AC0.10.3 tracker initialisation blocks the main thread for less than 50 ms', async () => {
    const { ctx, page } = await open();
    const init = await page.evaluate(() => window.__t1 - window.__t0);
    const long = await page.evaluate(() => window.__long.reduce((a, b) => a + b, 0));
    console.log(`init ${init.toFixed(1)} ms (incl. localhost fetch), long tasks total ${long.toFixed(1)} ms`);
    assert.ok(init < 50, `init ${init} ms`);
    assert.ok(long < 50, `long tasks ${long} ms`);
    assert.ok(await page.evaluate(() => typeof window.greprum === 'object'));
    await ctx.close();
  });

  it('AC0.10.3 fifty clicks never block the main thread for 50 ms', async () => {
    const { ctx, page } = await open();
    const worst = await page.evaluate(() => {
      const btns = [...document.querySelectorAll('button.buy')];
      let max = 0;
      for (let i = 0; i < 50; i++) {
        const t = performance.now();
        btns[i % btns.length].dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: 10 + i * 40, clientY: 10 }));
        max = Math.max(max, performance.now() - t);
      }
      return max;
    });
    console.log(`slowest click handler ${worst.toFixed(2)} ms`);
    assert.ok(worst < 50, `${worst} ms`);
    await ctx.close();
  });

  it('AC0.10.3 pending events are flushed when the tab closes', async () => {
    bodies = [];
    const { ctx, page } = await open();
    await page.evaluate(() => window.greprum.track('closing_event', { via: 'test' }));
    await page.close({ runBeforeUnload: true });
    for (let i = 0; i < 40 && !bodies.some((b) => b.events.some((e) => e.name === 'closing_event')); i++) {
      await new Promise((r) => setTimeout(r, 50));
    }
    const names = bodies.flatMap((b) => b.events.map((e) => e.name));
    assert.ok(names.includes('closing_event'), `got ${names}`);
    assert.ok(names.includes('$session_start'));
    await ctx.close();
  });
});
