import { api } from './api';
import { go, paperHref } from './router.svelte';
import { app } from './state.svelte';
import { toasts } from './toast.svelte';

/** Add PDFs dropped or picked, one at a time (each is extracted), saying how each went. */
export async function uploadPdfs(files: File[]) {
  const pdfs = files.filter((f) => f.name.toLowerCase().endsWith('.pdf'));
  if (pdfs.length < files.length) toasts.show('info', 'Only PDFs are added', `${files.length - pdfs.length} other file(s) skipped`);
  let last: string | undefined;
  for (const file of pdfs) {
    toasts.show('info', `Adding ${file.name}`, 'Looking up its metadata and extracting its text…');
    try {
      const { id, messages } = await api.upload(file);
      last = id;
      toasts.show('success', `Added ${id}`, messages.join('\n') || undefined);
    } catch (e) {
      toasts.error(e, `Cannot add ${file.name}`);
    }
  }
  app.changed();
  if (pdfs.length === 1 && last) location.hash = paperHref(last).slice(1);
}

/** Add by DOI, arXiv id, URL or title. Returns the new id. */
export async function addSource(source: string, open = true): Promise<string | undefined> {
  try {
    const { id, messages } = await api.add(source);
    toasts.show('success', `Added ${id}`, messages.join('\n') || undefined);
    app.changed();
    if (open) go(`/paper/${encodeURIComponent(id)}`);
    return id;
  } catch (e) {
    toasts.error(e, `Cannot add ${source}`);
  }
}
