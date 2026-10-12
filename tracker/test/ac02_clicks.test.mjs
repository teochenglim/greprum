import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { env } from './helpers.mjs';

const secret = 'hunter2-secret-value';

describe('AC0.2.2 click capture', () => {
  it('AC0.2.2 button click recorded with tag, id, text and css path', async () => {
    const t = env();
    t.click(t.$('#cta'));
    await t.flush();
    const c = t.named('$click')[0];
    assert.equal(c.element.tag, 'button');
    assert.equal(c.element.id, 'cta');
    assert.equal(c.element.text, 'Start free');
    assert.equal(c.element.css, 'main>button#cta');
  });

  it('AC0.2.2 div outside the allowlist is ignored, [data-rum-click] opts in', async () => {
    const t = env({ html: '<div id="a">a</div><div id="b" data-rum-click>b</div>' });
    t.click(t.$('#a'));
    t.click(t.$('#b'));
    await t.flush();
    const c = t.named('$click');
    assert.equal(c.length, 1);
    assert.equal(c[0].element.id, 'b');
  });

  it('AC0.2.2 ancestor match: click on a span inside a button records the button', async () => {
    const t = env({ html: '<button id="b"><span id="s">Buy</span></button>' });
    t.click(t.$('#s'));
    await t.flush();
    assert.equal(t.named('$click')[0].element.tag, 'button');
  });

  it('AC0.2.2 data-capture=all records every click', async () => {
    const t = env({ attrs: { 'data-capture': 'all' }, html: '<div id="a">a</div>' });
    t.click(t.$('#a'));
    await t.flush();
    assert.equal(t.named('$click').length, 1);
  });

  it('AC0.2.2 input, textarea and password values never appear in the payload', async () => {
    const t = env({
      html: `<input id="i" value="${secret}"><textarea id="x">${secret}</textarea><input id="p" type="password" value="${secret}"><select id="s"><option>${secret}</option></select>`,
    });
    for (const id of ['#i', '#x', '#p', '#s']) t.click(t.$(id));
    await t.flush();
    assert.ok(t.named('$click').length >= 4);
    assert.ok(!JSON.stringify(t.batches()).includes(secret));
  });

  it('AC0.2.2 text is cut to 50 chars and data-* attributes are captured', async () => {
    const t = env({ html: `<button id="b" data-plan="pro">${'x'.repeat(80)}</button>` });
    t.click(t.$('#b'));
    await t.flush();
    const e = t.named('$click')[0].element;
    assert.equal(e.text.length, 50);
    assert.equal(e.attrs['data-plan'], 'pro');
  });
});

describe('AC0.2.4 rage click', () => {
  const html = '<button id="b">Buy</button><button id="n">Next</button><textarea id="t"></textarea><button id="r" data-rum-no-rage>X</button>';
  const burst = (t, el, n = 3, gap = 100, dx = 0) => { for (let i = 0; i < n; i++) { t.click(el, { x: 50 + i * dx, y: 50 }); t.advance(gap); } };

  it('AC0.2.4 three fast clicks within 30 px produce exactly one $rage_click', async () => {
    const t = env({ html });
    burst(t, t.$('#b'), 5);
    await t.flush();
    assert.equal(t.named('$rage_click').length, 1);
    assert.equal(t.named('$rage_click')[0].element.id, 'b');
  });

  it('AC0.2.4 two clicks are not a rage click', async () => {
    const t = env({ html });
    burst(t, t.$('#b'), 2);
    await t.flush();
    assert.equal(t.named('$rage_click').length, 0);
  });

  it('AC0.2.4 clicks more than 30 px apart or slower than 1 s are not a rage click', async () => {
    const far = env({ html });
    burst(far, far.$('#b'), 3, 100, 40);
    await far.flush();
    assert.equal(far.named('$rage_click').length, 0);
    const slow = env({ html });
    burst(slow, slow.$('#b'), 3, 600);
    await slow.flush();
    assert.equal(slow.named('$rage_click').length, 0);
  });

  it('AC0.2.4 next/previous text, textarea and data-rum-no-rage are ignored', async () => {
    const t = env({ html });
    for (const id of ['#n', '#t', '#r']) burst(t, t.$(id), 4);
    await t.flush();
    assert.equal(t.named('$rage_click').length, 0);
  });

  it('AC0.2.4 a second burst after a pause fires again', async () => {
    const t = env({ html });
    burst(t, t.$('#b'), 3);
    t.advance(1500);
    burst(t, t.$('#b'), 3);
    await t.flush();
    assert.equal(t.named('$rage_click').length, 2);
  });
});

describe('AC0.2.5 dead click', () => {
  it('AC0.2.5 click with no DOM mutation, scroll, selection or URL change is dead after 1 s', async () => {
    const t = env();
    t.click(t.$('#cta'));
    t.advance(1100);
    await t.flush();
    assert.equal(t.named('$dead_click').length, 1);
  });

  it('AC0.2.5 a DOM mutation within the window means not dead', async () => {
    const t = env();
    t.click(t.$('#cta'));
    t.$('#plain').textContent = 'changed';
    await t.tick();
    t.advance(1100);
    await t.flush();
    assert.equal(t.named('$dead_click').length, 0);
  });

  it('AC0.2.5 links are never dead and data-rum-no-dead is skipped', async () => {
    const t = env({ html: '<a id="l" href="/x">go</a><button id="n" data-rum-no-dead>n</button>' });
    t.click(t.$('#l'));
    t.click(t.$('#n'));
    t.advance(1100);
    await t.flush();
    assert.equal(t.named('$dead_click').length, 0);
  });

  it('AC0.2.5 a URL change within the window means not dead', async () => {
    const t = env();
    t.click(t.$('#cta'));
    t.window.history.pushState({}, '', '/after');
    t.advance(1100);
    await t.flush();
    assert.equal(t.named('$dead_click').length, 0);
  });
});
