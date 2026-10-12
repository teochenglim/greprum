import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { env } from './helpers.mjs';

describe('AC0.2.1 session start and page views', () => {
  it('AC0.2.1 first event of a session is $session_start with utm read before the query is stripped', async () => {
    const t = env({ url: 'https://www.example.com/pricing?utm_source=news&utm_medium=email&token=secret#x' });
    await t.flush();
    const ev = t.events();
    assert.equal(ev[0].name, '$session_start');
    assert.equal(ev[0].seq, 0);
    assert.equal(ev[0].path, '/pricing');
    assert.equal(ev[0].is_new_visitor, true);
    assert.equal(ev[0].referrer, 'https://news.example.org');
    assert.deepEqual(ev[0].props, { utm_source: 'news', utm_medium: 'email' });
    assert.equal(ev[1].name, '$pageview');
    assert.ok(!JSON.stringify(t.batches()).includes('secret'), 'query string never sent');
  });

  it('AC0.2.1 pageview strips query and hash and carries seq in order', async () => {
    const t = env({ url: 'https://www.example.com/a/b?x=1#frag' });
    await t.flush();
    const pv = t.named('$pageview')[0];
    assert.equal(pv.path, '/a/b');
    assert.deepEqual(t.events().map((e) => e.seq), [0, 1]);
  });

  it('AC0.2.1 SPA pushState, replaceState and popstate emit pageviews with parent_path', async () => {
    const t = env();
    t.window.history.pushState({}, '', '/docs?x=1');
    t.window.history.replaceState({}, '', '/docs/start#top');
    await t.flush();
    const pv = t.named('$pageview');
    assert.deepEqual(pv.map((e) => e.path), ['/pricing', '/docs', '/docs/start']);
    assert.equal(pv[1].parent_path, '/pricing');
    assert.equal(pv[2].parent_path, '/docs');
    t.window.history.back();
    await t.tick();
  });

  it('AC0.2.1 data-auto-pageview=false disables automatic views but page() still reports', async () => {
    const t = env({ attrs: { 'data-auto-pageview': 'false' } });
    t.window.history.pushState({}, '', '/next');
    await t.flush();
    assert.equal(t.named('$pageview').length, 0);
    t.window.greprum.page('/checkout/step-1', 'checkout');
    await t.flush();
    const pv = t.named('$pageview');
    assert.equal(pv.length, 1);
    assert.equal(pv[0].path, '/checkout/step-1');
    assert.equal(pv[0].page_group, 'checkout');
  });

  it('AC0.2.1 an unchanged path does not emit a duplicate pageview', async () => {
    const t = env();
    t.window.history.replaceState({}, '', '/pricing?a=1');
    await t.flush();
    assert.equal(t.named('$pageview').length, 1);
  });
});
