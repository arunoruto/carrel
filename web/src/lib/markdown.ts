import DOMPurify from 'dompurify';
import { marked } from 'marked';

/** Markdown (summaries, notes, full texts) as safe HTML: papers come from anywhere. */
export function markdown(text: string, base?: (src: string) => string): string {
  let html = marked.parse(text, { async: false, gfm: true }) as string;
  if (base) html = html.replace(/(<img[^>]*\ssrc=")(?!https?:|data:|\/)([^"]+)"/g, (_, start, src) => `${start}${base(src)}"`);
  return DOMPurify.sanitize(html, FORBID);
}

/** Drafts come from any AI over MCP: no controls or styles that could dress up as this page's own. */
const FORBID = { FORBID_TAGS: ['form', 'button', 'input', 'textarea', 'select', 'style'], FORBID_ATTR: ['style'] };

/** "Vaswani, Shazeer, Parmar et al." */
export function authors(list: string[], max = 3): string {
  const family = (a: string) => (a.includes(',') ? a.split(',')[0] : a.split(' ').at(-1)) ?? a;
  if (list.length === 0) return '';
  const names = list.slice(0, max).map(family);
  return list.length > max ? `${names.join(', ')} et al.` : names.join(', ');
}

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1 << 20) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / (1 << 20)).toFixed(1)} MB`;
}

export function ago(secs: number): string {
  const d = Date.now() / 1000 - secs;
  if (d < 90) return 'just now';
  if (d < 5400) return `${Math.round(d / 60)} min ago`;
  if (d < 129600) return `${Math.round(d / 3600)} h ago`;
  if (d < 86400 * 45) return `${Math.round(d / 86400)} days ago`;
  return new Date(secs * 1000).toLocaleDateString();
}

/** "Hapke, Wells · Icarus · 1981" */
export function byline(r: { authors?: string[]; venue?: string | null; year?: number | null }): string {
  return [authors(r.authors ?? []), r.venue, r.year].filter(Boolean).join(' · ');
}
