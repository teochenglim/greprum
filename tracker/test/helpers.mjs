import { JSDOM } from 'jsdom';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
export const code = (name = 't.js') => readFileSync(join(here, '..', 'dist', name), 'utf8');
export const T0 = 1_760_000_000_000;

/** Boot the tracker in jsdom with a controllable clock, fetch and beacon. */
export function env({
  attrs = {},
  html = '<main><button id="cta">Start free</button><a href="/x" id="lnk">Go</a><div id="plain">p</div></main>',
  url = 'https://www.example.com/pricing',
  referrer = 'https://news.example.org/post?x=1',
  file = 't.js',
  storage,
  fetchStatus = 200,
  beaconOk = true,
  dnt,
  boot = true,
} = {}) {
  const a = { 'data-site': 'pk_test', src: 'https://t.example.com/t.js', ...attrs };
  const attrStr = Object.entries(a).map(([k, v]) => `${k}="${String(v).replace(/"/g, '&quot;')}"`).join(' ');
  const dom = new JSDOM(`<!doctype html><html><body>${html}<script ${attrStr}></script></body></html>`, {
    url, referrer, runScripts: 'outside-only', pretendToBeVisual: true,
  });
  const w = dom.window;
  let now = T0;
  const timers = [];
  let tid = 1;
  w.setTimeout = (fn, ms = 0) => { timers.push({ id: tid, at: now + ms, fn }); return tid++; };
  w.clearTimeout = (id) => { const i = timers.findIndex((t) => t.id === id); if (i > -1) timers.splice(i, 1); };
  w.Date = class extends Date { static now() { return now; } };
  const fetches = [], beacons = [];
  const state = { fetchStatus, fail: false, beaconOk };
  w.fetch = (u, o) => {
    const rec = { url: u, opts: o, body: JSON.parse(o.body) };
    fetches.push(rec);
    return state.fail ? Promise.reject(new Error('net')) : Promise.resolve({ status: state.fetchStatus });
  };
  w.Blob = class { constructor(p, o) { this.text = p.join(''); this.type = o && o.type; } };
  w.navigator.sendBeacon = (u, blob) => { beacons.push({ url: u, body: JSON.parse(blob.text), type: blob.type }); return state.beaconOk; };
  if (dnt) Object.defineProperty(w.navigator, 'doNotTrack', { value: dnt });
  if (storage) for (const [k, v] of Object.entries(storage)) w.localStorage.setItem(k, v);

  const api = {
    window: w, document: w.document, state, fetches, beacons,
    run() { w.eval(code(file)); },
    advance(ms) {
      const end = now + ms;
      for (;;) {
        timers.sort((x, y) => x.at - y.at);
        if (!timers.length || timers[0].at > end) break;
        const t = timers.shift();
        now = Math.max(now, t.at);
        t.fn();
      }
      now = end;
    },
    get now() { return now; },
    async tick() { await Promise.resolve(); await Promise.resolve(); },
    async flush() { api.advance(1100); await api.tick(); },
    batches() { return [...fetches.map((f) => f.body), ...beacons.map((b) => b.body)]; },
    events() { return api.batches().flatMap((b) => b.events); },
    named(n) { return api.events().filter((e) => e.name === n); },
    click(el, { x = 10, y = 10 } = {}) {
      el.dispatchEvent(new w.MouseEvent('click', { bubbles: true, cancelable: true, clientX: x, clientY: y }));
    },
    hide() {
      Object.defineProperty(w.document, 'visibilityState', { value: 'hidden', configurable: true });
      w.document.dispatchEvent(new w.Event('visibilitychange'));
    },
    $: (sel) => w.document.querySelector(sel),
  };
  if (boot) api.run();
  return api;
}
