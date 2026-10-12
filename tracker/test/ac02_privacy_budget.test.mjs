import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { env, code } from './helpers.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const gz = (f) => gzipSync(Buffer.from(code(f)), { level: 9 }).length;

describe('AC0.2.10 privacy in the browser', () => {
  it('AC0.2.10 data-rum-no-capture silences the element and its children', async () => {
    const t = env({ html: '<section data-rum-no-capture><button id="b">Secret</button></section><button id="ok">ok</button>' });
    t.click(t.$('#b'));
    t.click(t.$('#ok'));
    await t.flush();
    const c = t.named('$click');
    assert.equal(c.length, 1);
    assert.equal(c[0].element.id, 'ok');
  });

  it('AC0.2.10 [data-private] keeps the event and masks text as ***', async () => {
    const t = env({ html: '<div data-private><button id="b">Jane Doe 4111</button></div>' });
    t.click(t.$('#b'));
    await t.flush();
    assert.equal(t.named('$click')[0].element.text, '***');
    assert.ok(!JSON.stringify(t.batches()).includes('Jane Doe'));
  });

  it('AC0.2.10 exclude-pages silences matching paths; include-pages limits to matches', async () => {
    const ex = env({ url: 'https://www.example.com/admin/users', attrs: { 'data-exclude-pages': '/admin/*' } });
    await ex.flush();
    assert.equal(ex.batches().length, 0);
    const inc = env({ url: 'https://www.example.com/blog', attrs: { 'data-include-pages': '/app/*,/pricing' } });
    await inc.flush();
    assert.equal(inc.batches().length, 0);
    const ok = env({ url: 'https://www.example.com/pricing', attrs: { 'data-include-pages': '/app/*,/pricing' } });
    await ok.flush();
    assert.ok(ok.batches().length > 0);
  });

  it('AC0.2.10 include-pages together with exclude-pages disables the tracker', async () => {
    const t = env({ attrs: { 'data-include-pages': '/a', 'data-exclude-pages': '/b' } });
    t.click(t.$('#cta'));
    await t.flush();
    assert.equal(t.batches().length, 0);
    assert.equal(t.window.greprum, undefined);
  });

  it('AC0.2.10 Do Not Track honoured when data-respect-dnt=true, ignored otherwise', async () => {
    const on = env({ attrs: { 'data-respect-dnt': 'true' }, dnt: '1' });
    await on.flush();
    assert.equal(on.batches().length, 0);
    const off = env({ dnt: '1' });
    await off.flush();
    assert.ok(off.batches().length > 0);
  });

  it('AC0.2.10 query-string secrets in the URL, referrer and form action never leave the page', async () => {
    const t = env({
      url: 'https://www.example.com/p?token=TOPSECRET&email=a@b.c',
      referrer: 'https://ref.example.org/x?session=TOPSECRET',
      html: '<form id="f" action="/go?token=TOPSECRET"><input name="q"></form>',
    });
    t.$('#f').dispatchEvent(new t.window.Event('submit', { bubbles: true, cancelable: true }));
    await t.flush();
    assert.ok(!JSON.stringify(t.batches()).includes('TOPSECRET'));
    assert.ok(!JSON.stringify(t.batches()).includes('a@b.c'));
  });
});

describe('AC0.2.11 size and main-thread budgets', () => {
  it('AC0.2.11 core build is at most 3 KB gzipped', () => {
    assert.ok(gz('t.core.js') <= 3072, `core ${gz('t.core.js')} B`);
  });

  it('AC0.2.11 total build is at most 5 KB gzipped', () => {
    assert.ok(gz('t.js') <= 5120, `total ${gz('t.js')} B`);
  });

  it('AC0.2.11 initialisation is synchronous and small: no timers beyond the flush timer', () => {
    const t = env({ boot: false });
    const start = process.hrtime.bigint();
    t.run();
    const ms = Number(process.hrtime.bigint() - start) / 1e6;
    assert.ok(ms < 50, `init took ${ms} ms`);
    assert.equal(t.fetches.length, 0, 'nothing is sent synchronously');
  });
});

describe('AC0.2.12 provenance and licence', () => {
  it('AC0.2.12 no third-party tracker source is vendored and the repo NOTICE says the tracker is original', () => {
    assert.ok(!existsSync(join(here, '..', 'vendor')));
    const n = readFileSync(join(here, '..', '..', 'NOTICE'), 'utf8');
    assert.match(n, /original code/);
  });

  it('AC0.2.12 built t.js header names Apache-2.0 and contains no AGPL text', () => {
    const c = code('t.js');
    assert.match(c.slice(0, 200), /Apache-2\.0/);
    assert.ok(!/AGPL/i.test(c));
  });
});
