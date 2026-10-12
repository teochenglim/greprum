import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { env, T0 } from './helpers.mjs';

describe('AC0.2.7 identity and session', () => {
  it('AC0.2.7 ids persist across page loads in the same browser profile', async () => {
    const a = env();
    await a.flush();
    const vid = a.batches()[0].visitor_id;
    const b = env({ storage: { rum_v: vid } });
    await b.flush();
    assert.equal(b.batches()[0].visitor_id, vid);
    assert.equal(b.named('$session_start')[0].is_new_visitor, false);
  });

  it('AC0.2.7 31 minutes idle starts a new session with a new $session_start, 29 minutes does not', async () => {
    const t = env();
    await t.flush();
    const s1 = t.batches()[0].session_id;
    t.advance(29 * 60_000);
    t.window.greprum.track('still');
    await t.flush();
    assert.equal(t.batches().at(-1).session_id, s1);
    t.advance(31 * 60_000);
    t.window.greprum.track('later');
    await t.flush();
    const last = t.batches().at(-1);
    assert.notEqual(last.session_id, s1);
    assert.equal(last.events[0].name, '$session_start');
    assert.equal(last.events[0].seq, 0);
  });

  it('AC0.2.7 cookie mode stores both ids in a first-party cookie on the configured domain', async () => {
    const t = env({ attrs: { 'data-storage': 'cookie', 'data-cookie-domain': 'example.com' } });
    await t.flush();
    assert.match(t.document.cookie, /rum_v=v_/);
    assert.match(t.document.cookie, /rum_s=/);
    assert.equal(t.window.localStorage.length, 0);
  });

  it('AC0.2.7 default mode writes no cookie', async () => {
    const t = env();
    await t.flush();
    assert.equal(t.document.cookie, '');
    assert.ok(t.window.localStorage.getItem('rum_v'));
    assert.ok(t.window.sessionStorage.getItem('rum_s'));
  });
});

describe('AC0.2.8 sampling', () => {
  const draw = (v) => { const r = Math.random; return { set(w) { w.Math.random = () => v; }, restore() { Math.random = r; } }; };

  it('AC0.2.8 an unsampled session sends nothing at all', async () => {
    const t = env({ attrs: { 'data-sample-rate': '0.25' }, boot: false });
    t.window.Math.random = () => 0.9; // above the rate
    t.run();
    t.window.greprum.track('x');
    t.click(t.$('#cta'));
    t.window.history.pushState({}, '', '/p');
    t.hide();
    await t.flush();
    assert.equal(t.fetches.length + t.beacons.length, 0);
  });

  it('AC0.2.8 a sampled session sends events and every batch carries sample_rate', async () => {
    const t = env({ attrs: { 'data-sample-rate': '0.25' }, boot: false });
    t.window.Math.random = () => 0.1;
    t.run();
    await t.flush();
    assert.ok(t.events().length >= 2);
    assert.ok(t.batches().every((b) => b.sample_rate === 0.25));
  });

  it('AC0.2.8 the decision is stable for the whole session and invalid rates fall back to 1', async () => {
    const t = env({ attrs: { 'data-sample-rate': '0.5' }, boot: false });
    let n = 0;
    t.window.Math.random = () => (n++ < 3 ? 0.1 : 0.99); // visitor id, session id, sampling draw; later draws would not sample in
    t.run();
    t.window.greprum.track('a');
    t.window.greprum.track('b');
    await t.flush();
    assert.ok(t.named('a').length === 1 && t.named('b').length === 1);
    const bad = env({ attrs: { 'data-sample-rate': '7' } });
    await bad.flush();
    assert.equal(bad.batches()[0].sample_rate, 1);
  });
});

describe('AC0.2.9 delivery', () => {
  it('AC0.2.9 posts text/plain to the site-key URL with keepalive and no credentials', async () => {
    const t = env();
    await t.flush();
    const f = t.fetches[0];
    assert.equal(f.url, 'https://t.example.com/collect?k=pk_test');
    assert.equal(f.opts.headers['content-type'], 'text/plain');
    assert.equal(f.opts.keepalive, true);
    assert.equal(f.opts.credentials, 'omit');
  });

  it('AC0.2.9 seq is monotonic across batches and sent_at is set per batch', async () => {
    const t = env();
    await t.flush();
    t.window.greprum.track('a');
    await t.flush();
    t.window.greprum.track('b');
    await t.flush();
    const seqs = t.events().map((e) => e.seq);
    assert.deepEqual(seqs, seqs.map((_, i) => i));
    assert.ok(t.batches().every((b) => typeof b.sent_at === 'number' && b.sent_at >= T0));
  });

  it('AC0.2.9 a failed send is retried with the same seq values', async () => {
    const t = env();
    t.state.fail = true;
    await t.flush();
    assert.equal(t.fetches.length, 1);
    t.state.fail = false;
    t.advance(2100);
    await t.tick();
    assert.equal(t.fetches.length, 2);
    assert.deepEqual(t.fetches[1].body.events.map((e) => e.seq), t.fetches[0].body.events.map((e) => e.seq));
  });

  it('AC0.2.9 hidden tab flushes with sendBeacon (text/plain), falling back to fetch if refused', async () => {
    const t = env();
    t.window.greprum.track('bye');
    t.hide();
    assert.equal(t.beacons.length, 1);
    assert.equal(t.beacons[0].type, 'text/plain');
    assert.ok(t.beacons[0].body.events.some((e) => e.name === 'bye'));
    const r = env({ beaconOk: false });
    r.window.greprum.track('bye');
    r.hide();
    assert.equal(r.fetches.length, 1, 'beacon refused ⇒ keepalive fetch');
  });

  it('AC0.2.9 more than 50 queued events are split into batches of at most 50', async () => {
    const t = env();
    for (let i = 0; i < 120; i++) t.window.greprum.track('e' + (i % 9));
    await t.flush();
    assert.ok(t.batches().every((b) => b.events.length <= 50));
    assert.ok(t.events().length >= 120);
  });

  it('AC0.2.9 payload matches the gateway contract fields', async () => {
    const t = env({ attrs: { 'data-env': 'prod', 'data-release': '1.4.2' } });
    t.click(t.$('#cta'));
    await t.flush();
    const b = t.batches()[0];
    for (const k of ['token', 'visitor_id', 'session_id', 'sent_at', 'sample_rate', 'env', 'release', 'ctx', 'events']) assert.ok(k in b, k);
    assert.equal(b.env, 'prod');
    assert.equal(b.release, '1.4.2');
    const c = t.named('$click')[0];
    for (const k of ['ts', 'seq', 'name', 'path', 'parent_path', 'page_group', 'element']) assert.ok(k in c, k);
  });
});
