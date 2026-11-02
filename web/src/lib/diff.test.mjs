// node web/src/lib/diff.test.mjs (Node 23+, which strips the TypeScript types)
import { diff } from './diff.ts';
const d = diff('a\nb\nc\nd\ne\nf\ng\nh\ni\nj', 'a\nb\nc\nd\ne\nF\ng\nh\ni\nj');
const kinds = d.map((l) => l.kind).join('');
if (kinds !== '…   -+   …') throw new Error(`unexpected: ${JSON.stringify(kinds)}`);
if (d[0].skipped !== 2) throw new Error('two lines folded before the change');
console.log('diff ok:', JSON.stringify(kinds));
