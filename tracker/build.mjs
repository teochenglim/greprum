// Builds dist/t.js (all features compiled in, activated by data-features) and
// dist/t.core.js (features compiled out) for the size budget.
import { build } from 'esbuild';
import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
mkdirSync(join(here, 'dist'), { recursive: true });
const banner = '/*! GrepRUM tracker - Apache-2.0 - https://github.com/teochenglim/greprum */';

for (const [out, features] of [['t.js', true], ['t.core.js', false]]) {
  await build({
    entryPoints: [join(here, 'src', 't.js')],
    outfile: join(here, 'dist', out),
    bundle: false,
    minify: true,
    target: 'es2017',
    legalComments: 'none',
    drop: ['console'],
    banner: { js: banner },
    define: { __FEATURES__: String(features), __EXTRAS__: String(features) },
  });
}
console.log('tracker built');
