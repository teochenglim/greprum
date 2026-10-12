// GrepRUM tracker (original code, Apache-2.0). Optional features are compiled in only when
// __FEATURES__ is true and activated only when listed in data-features. __EXTRAS__ gates the
// default-on frustration/form events (rage, dead, form submit), which sit outside the 3 KB core.
(function (w, d) {
  var s = d.currentScript || d.querySelector('script[data-site]');
  if (!s) return;
  var A = function (n, def) { var v = s.getAttribute('data-' + n); return v == null ? def : v; };
  var site = A('site');
  if (!site) return;
  var nav = w.navigator, loc = w.location, ls, ss;
  var feats = ',' + A('features', '') + ',';
  var F = function (n) { return feats.indexOf(',' + n + ',') > -1; };
  var inc = A('include-pages'), exc = A('exclude-pages');
  if (inc && exc) { return; }
  if (A('respect-dnt') === 'true' && (nav.doNotTrack === '1' || w.doNotTrack === '1')) return;
  var rate = parseFloat(A('sample-rate', '1'));
  if (!(rate > 0 && rate <= 1)) rate = 1;
  var origin = (function () { try { return new URL(s.src).origin; } catch (e) { return ''; } })();
  var url = (A('endpoint') || origin + '/collect') + '?k=' + encodeURIComponent(site);
  var needConsent = A('consent') === 'required', consented = !needConsent, denied = false;
  var cookieMode = A('storage') === 'cookie', cdom = A('cookie-domain', '');
  var captureAll = A('capture') === 'all';
  var autoPV = A('auto-pageview') !== 'false';
  var IDLE = 30 * 60 * 1000, MAXB = 50;
  var q = [], ctx = {}, token = '', timer, mem = {}, page = { id: '', group: '' }, cur = '';
  var NAME = /^[a-z0-9_.:-]{1,64}$/;

  // ---- storage: localStorage / sessionStorage, cookie mode, in-memory fallback ----
  try { ls = w.localStorage; ss = w.sessionStorage; } catch (e) {}
  function get(k, sess) {
    try {
      if (cookieMode) { var m = d.cookie.match(new RegExp('(?:^|; )' + k + '=([^;]*)')); return m ? decodeURIComponent(m[1]) : mem[k]; }
      return (sess ? ss : ls).getItem(k) || mem[k];
    } catch (e) { return mem[k]; }
  }
  function set(k, v, sess) { // v == null removes
    if (v == null) delete mem[k]; else mem[k] = v;
    try {
      if (cookieMode) d.cookie = k + '=' + encodeURIComponent(v || '') + ';path=/;SameSite=Lax' + (cdom ? ';domain=' + cdom : '') + (v != null ? ';max-age=' + (sess ? 1800 : 31536000) : ';max-age=0');
      else if (v == null) { ls.removeItem(k); ss.removeItem(k); } else (sess ? ss : ls).setItem(k, v);
    } catch (e) {}
  }
  function rid(p) { return p + Math.random().toString(36).slice(2, 12); }
  function cut(v, n) { v = String(v == null ? '' : v); return v.length > n ? v.slice(0, n) : v; }
  function pathOf() { return page.id || loc.pathname; }

  // ---- identity + session (30 min idle, one sampling draw per session) ----
  var visitor, S; // S = {id, t (last activity), n (next seq), sm (sampled), pp (parent path)}
  function load() {
    visitor = get('rum_v');
    try { S = JSON.parse(get('rum_s', 1) || 'null'); } catch (e) { S = null; }
  }
  function session(now) {
    if (!consented || denied) return null;
    if (!visitor) { visitor = rid('v_'); set('rum_v', visitor); S = null; S0.fresh = 1; } else S0.fresh = 0;
    if (!S || now - S.t > IDLE) {
      S = { id: rid('s_'), t: now, n: 0, sm: Math.random() < rate ? 1 : 0, pp: '' };
      S0.start = 1;
    }
    S.t = now;
    set('rum_s', JSON.stringify(S), 1);
    return S;
  }
  var S0 = { fresh: 0, start: 0 };

  // ---- queueing and delivery ----
  function emit(name, extra) {
    if (!consented || denied) return;
    var now = Date.now(), sn = session(now);
    if (!sn || !sn.sm) return;
    var p = pathOf();
    if (inc && !match(inc, p)) return;
    if (exc && match(exc, p)) return;
    if (S0.start) {
      S0.start = 0;
      var u = {}, sp = new URLSearchParams(loc.search);
      ['utm_source', 'utm_medium', 'utm_campaign'].forEach(function (k) { var v = sp.get(k); if (v) u[k] = cut(v, 100); });
      var ref = '';
      try { ref = d.referrer ? new URL(d.referrer).origin : ''; } catch (e) {}
      push('$session_start', { is_new_visitor: !!S0.fresh, referrer: ref, props: u }, now, p);
    }
    push(name, extra, now, p);
  }
  function push(name, extra, now, p) {
    var e = { ts: now, seq: S.n++, name: name, path: p, parent_path: S.pp || '', page_group: page.group };
    for (var k in extra) e[k] = extra[k];
    set('rum_s', JSON.stringify(S), 1);
    q.push(e);
    if (q.length >= MAXB) flush(); else if (!timer) timer = setTimeout(flush, 1000);
  }
  function body(evs) {
    return JSON.stringify({
      token: token, visitor_id: visitor, session_id: S && S.id, sent_at: Date.now(), sample_rate: rate,
      env: A('env', ''), release: A('release', ''), ctx: ctx, events: evs
    });
  }
  function flush(beacon) {
    clearTimeout(timer); timer = 0;
    while (q.length) send(q.splice(0, MAXB), beacon, 0);
  }
  function send(evs, beacon, tries) {
    var b = body(evs);
    try {
      if (beacon && nav.sendBeacon && nav.sendBeacon(url, new Blob([b], { type: 'text/plain' }))) return;
    } catch (e) {}
    try {
      var r = w.fetch(url, { method: 'POST', body: b, keepalive: true, credentials: 'omit', headers: { 'content-type': 'text/plain' } });
      r.then(function (res) { if (res.status >= 500 && tries < 3) retry(evs, tries); }, function () { if (tries < 3) retry(evs, tries); });
    } catch (e) {}
  }
  function retry(evs, tries) { setTimeout(function () { send(evs, 0, tries + 1); }, 2000 * (tries + 1)); } // same seq values

  function match(list, p) {
    return list.split(',').some(function (g) {
      g = g.trim();
      return new RegExp('^' + g.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*') + '$').test(p);
    });
  }

  // ---- pages ----
  function pageview() {
    var p = pathOf();
    if (p === cur) return;
    if (cur && F2.leave) F2.leave();
    var prev = cur;
    cur = p;
    if (S) S.pp = prev; // parent_path: previous page in the session
    if (F2.reset) F2.reset();
    if (autoPV || page.id) emit('$pageview', {});
  }
  var F2 = {}; // optional feature hooks

  // ---- element description (never values) ----
  var CLICKABLE = 'a,button,form,input,select,textarea,label,[role=button],[role=link],[role=tab],[role=menuitem],[data-track],[data-rum-click]';
  function priv(el) { return !!el.closest('[data-private]'); }
  function css(el) {
    var out = [], n = 0;
    while (el && el.nodeType === 1 && n++ < 4 && el !== d.body) {
      var t = el.tagName.toLowerCase();
      if (el.id) t += '#' + cut(el.id, 40);
      else if (el.classList && el.classList[0]) t += '.' + cut(el.classList[0], 30);
      out.unshift(t);
      el = el.parentElement;
    }
    return out.join('>');
  }
  function describe(el) {
    var tag = el.tagName.toLowerCase(), a = {}, i, at;
    var valued = /^(input|textarea|select)$/.test(tag);
    for (i = 0; i < el.attributes.length && i < 20; i++) {
      at = el.attributes[i];
      if (at.name.indexOf('data-') === 0 && at.name.indexOf('data-rum-') !== 0 && at.name !== 'data-private') a[at.name] = cut(at.value, 50);
    }
    return {
      tag: tag, id: cut(el.id, 128), role: cut(el.getAttribute('role'), 32),
      text: valued ? '' : priv(el) ? '***' : cut((el.innerText || el.textContent || '').replace(/\s+/g, ' ').trim(), 50),
      css: css(el), attrs: a
    };
  }

  // ---- clicks: allowlist walk ----
  var X = {}; // extras (rage, dead, form submit): compiled out of the core build
  function onClick(ev) {
    var t = ev.target;
    if (!t || t.nodeType !== 1 || t.closest('[data-rum-no-capture]')) return;
    var el = captureAll ? t : t.closest(CLICKABLE);
    if (el) {
      emit('$click', { element: describe(el) });
      var tn = el.closest('[data-track]');
      if (tn) track(tn.getAttribute('data-track'));
    }
    if (X.click) X.click(ev, t, el || t);
  }

  if (__EXTRAS__) {
    var clicks = [], burst = 0, pend = [], mo;
    var NEXT = /^(next|previous|prev|>|<|\+|-)$/;
    var alive = function () { for (var i = 0; i < pend.length; i++) pend[i].a = 1; };
    var ignoreRage = function (el) {
      var x = (el.innerText || el.textContent || '').trim().toLowerCase(), a = (el.getAttribute('aria-label') || '').toLowerCase();
      return el.closest('[data-rum-no-rage],textarea,[contenteditable=""],[contenteditable=true],input:not([type=button]):not([type=submit]):not([type=checkbox]):not([type=radio])') ||
        NEXT.test(x) || NEXT.test(a) || /\b(next|previous|prev)\b/.test(x + ' ' + a);
    };
    // rage: 3 clicks within 30 px inside 1000 ms, one event per burst; dead: nothing observable in 1 s
    X.click = function (ev, t, el) {
      var now = Date.now(), c = { t: now, x: ev.clientX, y: ev.clientY };
      clicks = clicks.filter(function (k) { return now - k.t <= 1000; });
      if (!clicks.length) burst = 0;
      clicks.push(c);
      var near = clicks.filter(function (k) { return Math.abs(k.x - c.x) <= 30 && Math.abs(k.y - c.y) <= 30; });
      if (near.length >= 3 && !burst && !ignoreRage(t)) { burst = 1; emit('$rage_click', { element: describe(el) }); }
      if (t.closest('a[href],[data-rum-no-dead]')) return; // links are never dead
      var rec = { a: 0 }, u0 = loc.href;
      pend.push(rec);
      if (!mo && w.MutationObserver) { mo = new MutationObserver(alive); mo.observe(d, { subtree: true, childList: true, attributes: true, characterData: true }); }
      setTimeout(function () {
        pend.splice(pend.indexOf(rec), 1);
        if (!pend.length && mo) { mo.disconnect(); mo = 0; }
        if (!rec.a && loc.href === u0 && !(w.getSelection && String(w.getSelection()))) emit('$dead_click', { element: describe(el) });
      }, 1000);
    };
    X.submit = function (ev) {
      var f = ev.target;
      if (!f || f.nodeType !== 1 || f.closest('[data-rum-no-capture]')) return;
      alive();
      var names = [], i, el;
      for (i = 0; i < f.elements.length && names.length < 30; i++) { el = f.elements[i]; if (el.name) names.push(cut(el.name, 40)); }
      var act = ''; try { act = new URL(f.action, loc.href).pathname; } catch (e) {}
      emit('$form_submit', { props: { form_id: cut(f.id, 64), action: act, fields: names } });
    };
  }

  // ---- public API ----
  function track(name, props) {
    name = String(name || '').toLowerCase();
    if (!NAME.test(name)) return;
    var e = {};
    if (props && typeof props === 'object') { var j = JSON.stringify(props); if (j.length <= 4096) e.props = props; }
    emit(name, e);
  }
  function identify(tok) { token = String(tok || ''); emit('$identify', {}); flush(); }
  function reset() { set('rum_v'); set('rum_s'); visitor = S = null; token = ''; cur = ''; }
  function setContext(o) {
    if (!o || typeof o !== 'object') return;
    for (var k in o) {
      if (!ctx.hasOwnProperty(k) && Object.keys(ctx).length >= 10) continue;
      if (k.length <= 32) ctx[k] = cut(o[k], 200);
    }
  }
  function consent(v) {
    if (v === 'granted') { consented = true; denied = false; cur = ''; pageview(); }
    else if (v === 'denied') { denied = true; q = []; reset(); }
  }
  w.greprum = {
    track: track, identify: identify, reset: reset, setContext: setContext, consent: consent,
    page: function (id, group) { page = { id: id ? String(id) : '', group: cut(group, 64) }; cur = ''; pageview(); }
  };

  // ---- optional features: scroll, pageleave, errors, vitals ----
  if (__FEATURES__) {
    var t0 = Date.now(), fired = {}, left = 0, inp = 0;
    F2.reset = function () { fired = {}; left = 0; t0 = Date.now(); };
    if (F('scroll')) {
      d.addEventListener('scroll', function () {
        var de = d.documentElement, h = de.scrollHeight || 1;
        var depth = ((w.pageYOffset || de.scrollTop || 0) + (w.innerHeight || de.clientHeight || 0)) * 100 / h;
        [25, 50, 75, 100].forEach(function (m) {
          if (depth >= m && !fired[m]) { fired[m] = 1; emit('$scroll', { props: { depth: m } }); }
        });
      }, { passive: true });
    }
    if (F('pageleave')) {
      F2.leave = function () {
        if (left) return;
        left = 1;
        emit('$pageleave', { props: { duration_ms: Date.now() - t0 } });
      };
    }
    if (F('errors')) {
      w.addEventListener('error', function (e) {
        emit('$error', { props: { message: cut(e.message, 200), stack: cut(e.error && e.error.stack, 500) } });
      });
      w.addEventListener('unhandledrejection', function (e) {
        var r = e.reason;
        emit('$error', { props: { message: cut(r && r.message || r, 200), stack: cut(r && r.stack, 500) } });
      });
    }
    if (F('vitals') && w.PerformanceObserver) {
      try {
        new PerformanceObserver(function (l) {
          l.getEntries().forEach(function (e) { if (e.interactionId && e.duration > inp) inp = e.duration; });
        }).observe({ type: 'event', durationThreshold: 40, buffered: true });
      } catch (e) {}
      F2.vital = function () { if (inp) { emit('$web_vital', { props: { name: 'INP', value: Math.round(inp) } }); inp = 0; } };
    }
  }

  // ---- wire up ----
  load();
  function onRoute() { pageview(); }
  ['pushState', 'replaceState'].forEach(function (m) {
    var o = w.history[m];
    w.history[m] = function () { var r = o.apply(this, arguments); onRoute(); return r; };
  });
  w.addEventListener('popstate', onRoute);
  d.addEventListener('click', onClick, true);
  if (X.submit) d.addEventListener('submit', X.submit, true);
  d.addEventListener('visibilitychange', function () {
    if (d.visibilityState === 'hidden') {
      if (F2.leave) F2.leave();
      if (F2.vital) F2.vital();
      flush(1);
    }
  });
  w.addEventListener('pagehide', function () { if (F2.leave) F2.leave(); flush(1); });
  if (consented) pageview();
})(window, document);
