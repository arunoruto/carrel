// A line diff for the activity view: longest common subsequence, then long unchanged runs folded away.

export type Line = { kind: ' ' | '+' | '-'; text: string } | { kind: '…'; skipped: number };

/** Lines kept on each side of a change. */
const CONTEXT = 3;

export function diff(before: string, after: string): Line[] {
  const a = before.split('\n');
  const b = after.split('\n');
  // ponytail: O(n·m) table; past ~4M cells it shows the whole file replaced instead. Myers' diff if that bites.
  if (a.length * b.length > 4_000_000) return [...a.map((text) => ({ kind: '-' as const, text })), ...b.map((text) => ({ kind: '+' as const, text }))];
  const lcs = Array.from({ length: a.length + 1 }, () => new Uint32Array(b.length + 1));
  for (let i = a.length - 1; i >= 0; i--)
    for (let j = b.length - 1; j >= 0; j--) lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const lines: { kind: ' ' | '+' | '-'; text: string }[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) lines.push({ kind: ' ', text: a[i++] }), j++;
    else if (i < a.length && (j === b.length || lcs[i + 1][j] >= lcs[i][j + 1])) lines.push({ kind: '-', text: a[i++] });
    else lines.push({ kind: '+', text: b[j++] });
  }
  // Fold unchanged lines further than CONTEXT from any change.
  const near = lines.map((_, k) => lines.slice(Math.max(0, k - CONTEXT), k + CONTEXT + 1).some((l) => l.kind !== ' '));
  const out: Line[] = [];
  lines.forEach((line, k) => {
    if (near[k]) return out.push(line);
    const last = out.at(-1);
    if (last?.kind === '…') last.skipped++;
    else out.push({ kind: '…', skipped: 1 });
  });
  return out;
}
