// Size budget (PRD §3): core <= 3 KB gzipped, total <= 5 KB gzipped.
import { readFileSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const gz = (f) => gzipSync(readFileSync(join(here, 'dist', f)), { level: 9 }).length;
const core = gz('t.core.js'), total = gz('t.js');
console.log(`core ${core} B gz (budget 3072), total ${total} B gz (budget 5120)`);
if (core > 3072 || total > 5120) { console.error('tracker size budget exceeded'); process.exit(1); }
