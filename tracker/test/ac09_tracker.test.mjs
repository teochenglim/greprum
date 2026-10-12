import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { gzipSync } from 'node:zlib';
import { env, code } from './helpers.mjs';

const html = '<button id="b">Buy</button><button id="n">Next</button><button id="s">Skip</button>';
const burst = (t, el, n, gap = 100, dx = 0) => { for (let i = 0; i < n; i++) { t.click(el, { x: 50 + i * dx, y: 50 }); t.advance(gap); } };

describe('AC0.9.1 configurable rage click', () => {
  it('AC0.9.1 data-rage-count=5 needs five clicks', async () => {
    const four = env({ html, attrs: { 'data-rage-count': '5' } });
    burst(four, four.$('#b'), 4);
    await four.flush();
    assert.equal(four.named('$rage_click').length, 0);
    const five = env({ html, attrs: { 'data-rage-count': '5' } });
    burst(five, five.$('#b'), 5);
    await five.flush();
    assert.equal(five.named('$rage_click').length, 1);
  });

  it('AC0.9.1 data-rage-px=10 makes the radius stricter', async () => {
    const t = env({ html, attrs: { 'data-rage-px': '10' } });
    burst(t, t.$('#b'), 3, 100, 15); // 15 px apart: inside the default 30, outside 10
    await t.flush();
    assert.equal(t.named('$rage_click').length, 0);
    const d = env({ html });
    burst(d, d.$('#b'), 3, 100, 15);
    await d.flush();
    assert.equal(d.named('$rage_click').length, 1, 'default radius still fires');
  });

  it('AC0.9.1 data-rage-ms=300 ignores slower bursts', async () => {
    const t = env({ html, attrs: { 'data-rage-ms': '300' } });
    burst(t, t.$('#b'), 3, 200);
    await t.flush();
    assert.equal(t.named('$rage_click').length, 0);
    const f = env({ html, attrs: { 'data-rage-ms': '300' } });
    burst(f, f.$('#b'), 3, 50);
    await f.flush();
    assert.equal(f.named('$rage_click').length, 1);
  });

  it('AC0.9.1 data-rage-ignore replaces the ignore text list', async () => {
    const t = env({ html, attrs: { 'data-rage-ignore': 'skip' } });
    burst(t, t.$('#s'), 4);
    burst(t, t.$('#n'), 4); // "Next" is no longer ignored once the list is replaced
    await t.flush();
    const els = t.named('$rage_click').map((e) => e.element.id);
    assert.ok(!els.includes('s'));
    assert.ok(els.includes('n'));
  });

  it('AC0.9.1 defaults unchanged and size budget still holds', () => {
    const t = env({ html });
    burst(t, t.$('#n'), 4);
    assert.equal(t.named('$rage_click').length, 0, 'default list ignores Next');
    assert.ok(gzipSync(Buffer.from(code('t.js')), { level: 9 }).length <= 5120);
    assert.ok(gzipSync(Buffer.from(code('t.core.js')), { level: 9 }).length <= 3072);
  });
});

function withObserver(attrs) {
  const t = env({ attrs, boot: false });
  const made = [];
  t.window.PerformanceObserver = class {
    constructor(cb) { this.cb = cb; made.push(this); }
    observe() { this.observed = true; }
  };
  t.run();
  return { t, made };
}

describe('AC0.9.2 INP web vital', () => {
  it('AC0.9.2 reports the worst interaction duration once on hide', async () => {
    const { t, made } = withObserver({ 'data-features': 'vitals' });
    assert.equal(made.length, 1);
    made[0].cb({ getEntries: () => [{ interactionId: 1, duration: 120 }, { interactionId: 2, duration: 300 }, { duration: 900 }] });
    t.hide();
    t.hide();
    await t.tick();
    const v = t.named('$web_vital');
    assert.equal(v.length, 1);
    assert.deepEqual(v[0].props, { name: 'INP', value: 300 });
  });

  it('AC0.9.2 without the vitals feature no observer is created', async () => {
    const { t, made } = withObserver({});
    assert.equal(made.length, 0);
    t.hide();
    await t.tick();
    assert.equal(t.named('$web_vital').length, 0);
  });

  it('AC0.9.2 no interactions means no event', async () => {
    const { t, made } = withObserver({ 'data-features': 'vitals' });
    made[0].cb({ getEntries: () => [] });
    t.hide();
    await t.tick();
    assert.equal(t.named('$web_vital').length, 0);
  });
});
