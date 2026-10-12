# GrepRUM tracker

```html
<script async src="https://t.example.com/t.js" data-site="pk_xxx"
        data-features="scroll,errors,vitals,pageleave"
        data-sample-rate="0.25" data-env="prod" data-release="1.4.2"></script>
```

Core (page views, session start, SPA routes, clicks, `track()`, batching) ≤ 3 KB gzipped; total ≤ 5 KB.
`make tracker-size` enforces both.

## Attributes

| Attribute | Meaning |
|---|---|
| `data-site` | public site key `pk_xxx` (required) |
| `data-features` | comma list of `scroll`, `pageleave`, `errors`, `vitals` |
| `data-sample-rate` | per-session sampling, (0,1] |
| `data-env`, `data-release` | copied to every batch |
| `data-endpoint` | collect URL; default `<script origin>/collect` |
| `data-auto-pageview="false"` | disable automatic page views (use `greprum.page()`) |
| `data-capture="all"` | record every click, not only allowlisted elements |
| `data-consent="required"` | store and send nothing until `greprum.consent("granted")` |
| `data-respect-dnt="true"` | disable when Do Not Track is on |
| `data-storage="cookie"`, `data-cookie-domain` | first-party cookie on the top-level domain |
| `data-include-pages` / `data-exclude-pages` | path patterns with `*`; never both |

Markup: `data-track="name"`, `data-rum-click`, `data-rum-no-capture`, `data-rum-no-rage`,
`data-rum-no-dead`, `data-private` (text masked as `***`).

## API: `window.greprum`

`track(name, props)` · `page(id, group)` · `identify(signedToken)` · `reset()` ·
`setContext({k: v})` (max 10 keys) · `consent("granted"|"denied")`.

## Wire format

`POST <endpoint>?k=<site key>`, `Content-Type: text/plain`, JSON body (see `tests/golden/collect_payload.json`):

- batch: `token`, `visitor_id`, `session_id`, `sent_at`, `sample_rate`, `env`, `release`, `ctx`, `events[]` (≤ 50)
- event: `ts`, `seq` (per session, retried with the same value), `name`, `path`, `parent_path`, `page_group`, optional `element`, `props`, `is_new_visitor`, `referrer`

No IP, input values or query strings are ever sent (only the three `utm_` values, on session start).

## Develop

```
make tracker        # build dist/t.js and dist/t.core.js
make tracker-test   # node --test with jsdom
make tracker-size   # budget gate
```
