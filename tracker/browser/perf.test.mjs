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

  // Shared CI runners are 10-40x slower than a laptop, so an absolute 50 ms is meaningless there. A fixed
  // CPU workload calibrates the machine: the budget is 50 ms on the reference machine (REF_CALIB_MS) and
  // scales up only when the runner is slower than that.
  const REF_CALIB_MS = 35; // measured on the development laptop (Apple silicon, headless Chrome)
  async function budget(page) {
    const calib = await page.evaluate(() => {
      const run = () => {
        const t = performance.now();
        let x = 0;
        for (let i = 0; i < 4_000_000; i++) x += Math.sqrt(i) % 7;
        if (x < 0) console.log(x);
        return performance.now() - t;
      };
      return Math.min(run(), run(), run());
    });
    const scale = Math.max(1, calib / REF_CALIB_MS);
    return { calib, scale, ms: 50 * scale };
  }

  it('AC0.10.3 tracker initialisation blocks the main thread for less than 50 ms (machine-normalised)', async () => {
    const { ctx, page } = await open();
    const b = await budget(page);
    // Main-thread cost = from when the tracker script could start (its bytes had arrived and the
    // preceding inline script finished) to init done: parse + compile + run. Fetching is excluded.
    const m = await page.evaluate(() => {
      const r = performance.getEntriesByType('resource').find((e) => e.name.endsWith('/t.js'));
      return {
        exec: window.__t1 - Math.max(window.__t0, r.responseEnd),
        long: window.__long.reduce((a, c) => a + c, 0),
      };
    });
    console.log(`init exec ${m.exec.toFixed(1)} ms, long tasks ${m.long.toFixed(1)} ms; machine calib ${b.calib.toFixed(1)} ms (ref ${REF_CALIB_MS}), budget ${b.ms.toFixed(0)} ms`);
    assert.ok(m.exec < b.ms, `init execution ${m.exec} ms > budget ${b.ms} ms`);
    assert.ok(m.long < b.ms, `long tasks ${m.long} ms > budget ${b.ms} ms`);
    assert.ok(await page.evaluate(() => typeof window.greprum === 'object'));
    await ctx.close();
  });

  it('AC0.10.3 fifty clicks never block the main thread for 50 ms (machine-normalised)', async () => {
    const { ctx, page } = await open();
    const b = await budget(page);
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
    console.log(`slowest click handler ${worst.toFixed(2)} ms, budget ${b.ms.toFixed(0)} ms`);
    assert.ok(worst < b.ms, `${worst} ms > budget ${b.ms} ms`);
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
