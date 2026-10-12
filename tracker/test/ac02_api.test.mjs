import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { env } from './helpers.mjs';

describe('AC0.2.3 custom events and API', () => {
  it('AC0.2.3 data-track click emits the custom event as well as $click', async () => {
    const t = env({ html: '<button id="b" data-track="Signup_Click">go</button>' });
    t.click(t.$('#b'));
    await t.flush();
    assert.equal(t.named('signup_click').length, 1);
    assert.equal(t.named('$click').length, 1);
  });

  it('AC0.2.3 track() lowercases, refuses $ names and invalid names, caps props at 4 KB', async () => {
    const t = env();
    const g = t.window.greprum;
    g.track('Checkout.Done', { plan: 'pro' });
    g.track('$pageview');
    g.track('bad name');
    g.track('big', { v: 'x'.repeat(5000) });
    await t.flush();
    const names = t.events().map((e) => e.name);
    assert.ok(names.includes('checkout.done'));
    assert.equal(names.filter((n) => n === '$pageview').length, 1, 'only the automatic one');
    assert.ok(!names.includes('bad name'));
    assert.equal(t.named('big')[0].props, undefined);
    assert.deepEqual(t.named('checkout.done')[0].props, { plan: 'pro' });
  });

  it('AC0.2.3 identify sends the token with the current visitor_id', async () => {
    const t = env();
    t.window.greprum.identify('jwt.token.here');
    await t.tick();
    const b = t.batches().at(-1);
    assert.equal(b.token, 'jwt.token.here');
    assert.match(b.visitor_id, /^v_/);
    assert.equal(b.events.at(-1).name, '$identify');
  });

  it('AC0.2.3 reset rotates visitor and session ids and drops the token', async () => {
    const t = env();
    t.window.greprum.identify('tok');
    await t.tick();
    const before = t.batches().at(-1);
    t.window.greprum.reset();
    t.window.greprum.track('after');
    await t.flush();
    const after = t.batches().at(-1);
    assert.notEqual(after.visitor_id, before.visitor_id);
    assert.notEqual(after.session_id, before.session_id);
    assert.equal(after.token, '');
    assert.equal(after.events[0].name, '$session_start');
  });

  it('AC0.2.3 setContext keeps at most 10 keys and cuts values to 200', async () => {
    const t = env();
    const ctx = {};
    for (let i = 0; i < 15; i++) ctx['k' + i] = 'v';
    ctx.long = 'z'.repeat(300);
    t.window.greprum.setContext(ctx);
    t.window.greprum.track('x');
    await t.flush();
    const sent = t.batches().at(-1).ctx;
    assert.ok(Object.keys(sent).length <= 10);
    assert.ok(Object.values(sent).every((v) => v.length <= 200));
  });

  it('AC0.2.3 consent required: nothing stored or sent until granted, denied clears ids', async () => {
    const t = env({ attrs: { 'data-consent': 'required' } });
    t.window.greprum.track('early');
    await t.flush();
    assert.equal(t.batches().length, 0);
    assert.equal(t.window.localStorage.length, 0);
    t.window.greprum.consent('granted');
    t.window.greprum.track('late');
    await t.flush();
    assert.ok(t.named('late').length === 1 && t.named('$session_start').length === 1);
    assert.ok(t.window.localStorage.getItem('rum_v'));
    t.window.greprum.consent('denied');
    assert.equal(t.window.localStorage.getItem('rum_v'), null);
    const n = t.batches().length;
    t.window.greprum.track('blocked');
    await t.flush();
    assert.equal(t.batches().length, n);
  });
});

describe('AC0.2.6 optional features and form submit', () => {
  it('AC0.2.6 without data-features no scroll, error, vital or pageleave events exist', async () => {
    const t = env();
    t.window.dispatchEvent(new t.window.ErrorEvent('error', { message: 'boom' }));
    t.window.document.dispatchEvent(new t.window.Event('scroll'));
    t.hide();
    await t.flush();
    for (const n of ['$error', '$scroll', '$pageleave', '$web_vital']) assert.equal(t.named(n).length, 0, n);
  });

  it('AC0.2.6 scroll thresholds 25/50/75/100 fire once per page view', async () => {
    const t = env({ attrs: { 'data-features': 'scroll' } });
    const de = t.document.documentElement;
    Object.defineProperty(de, 'scrollHeight', { value: 1000, configurable: true });
    Object.defineProperty(t.window, 'innerHeight', { value: 500, configurable: true });
    for (const y of [0, 100, 600, 600, 1000]) {
      Object.defineProperty(t.window, 'pageYOffset', { value: y, configurable: true });
      t.document.dispatchEvent(new t.window.Event('scroll'));
    }
    await t.flush();
    assert.deepEqual(t.named('$scroll').map((e) => e.props.depth), [25, 50, 75, 100]);
    // a new page view resets the thresholds
    t.window.history.pushState({}, '', '/two');
    Object.defineProperty(t.window, 'pageYOffset', { value: 1000, configurable: true });
    t.document.dispatchEvent(new t.window.Event('scroll'));
    await t.flush();
    assert.equal(t.named('$scroll').length, 8, 'a new page view fires all four again');
  });

  it('AC0.2.6 pageleave carries duration_ms once on hidden', async () => {
    const t = env({ attrs: { 'data-features': 'pageleave' } });
    t.advance(5000);
    t.hide();
    t.hide();
    await t.tick();
    const pl = t.named('$pageleave');
    assert.equal(pl.length, 1);
    assert.equal(pl[0].props.duration_ms, 5000);
  });

  it('AC0.2.6 errors are reported with message and stack truncated', async () => {
    const t = env({ attrs: { 'data-features': 'errors' } });
    t.window.dispatchEvent(new t.window.ErrorEvent('error', { message: 'm'.repeat(500), error: { stack: 's'.repeat(900) } }));
    await t.flush();
    const e = t.named('$error')[0];
    assert.equal(e.props.message.length, 200);
    assert.equal(e.props.stack.length, 500);
  });

  it('AC0.2.6 form submit records form id, action path and field names but never values', async () => {
    const t = env({ html: '<form id="signup" action="/join?x=1"><input name="email" value="a@b.c"><input name="pw" type="password" value="hunter2"></form>' });
    t.$('#signup').dispatchEvent(new t.window.Event('submit', { bubbles: true, cancelable: true }));
    await t.flush();
    const f = t.named('$form_submit')[0];
    assert.deepEqual(f.props, { form_id: 'signup', action: '/join', fields: ['email', 'pw'] });
    assert.ok(!JSON.stringify(t.batches()).includes('hunter2'));
    assert.ok(!JSON.stringify(t.batches()).includes('a@b.c'));
  });
});
