<script lang="ts">
  import {
    ArrowLeft, BookOpen, Check, Copy, Download, ExternalLink, FileText, FileUp, Folder, FolderPlus, Image,
    Link, Network, NotebookPen, Paperclip, Quote, RefreshCw, ScanText, Trash2, X,
  } from '@lucide/svelte';
  import { api, type FieldName, type Paper } from '../lib/api';
  import Badge from '../lib/components/Badge.svelte';
  import Button from '../lib/components/Button.svelte';
  import Dialog from '../lib/components/Dialog.svelte';
  import Spinner from '../lib/components/Spinner.svelte';
  import Status from '../lib/components/Status.svelte';
  import { byline, markdown } from '../lib/markdown';
  import { go, paperHref, router } from '../lib/router.svelte';
  import { tick } from 'svelte';
  import { app } from '../lib/state.svelte';
  import { toasts } from '../lib/toast.svelte';
  import Connections from './Connections.svelte';
  import References from './References.svelte';

  // `reading`: the PDF over the whole area, at #/paper/<id>/read.
  let { id, reading = false }: { id: string; reading?: boolean } = $props();

  let paper = $state<Paper | null>(null);
  let error = $state('');
  let tab = $state<'overview' | 'notes' | 'text' | 'figures' | 'references' | 'cited' | 'connections' | 'bibtex'>('overview');
  let busy = $state<string | null>(null);
  let removing = $state(false);
  let notes = $state('');
  let editingNotes = $state(false);
  let keywordInput = $state('');
  let projectInput = $state('');
  let fulltext = $state<string | null>(null);
  let fullShown = $state(150_000);
  let copied = $state(false);
  let notesOpen = $state(false);
  let allAuthors = $state(false);
  let pdfPicker: HTMLInputElement;
  let suppPicker: HTMLInputElement;

  async function load() {
    try {
      paper = await api.paper(id);
      notes = paper.notes ?? '';
      error = '';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // `?at=12345`: a semantic search hit at that character of the full text.
  const at = $derived(Number(router.params.get('at')) || null);

  $effect(() => {
    void id;
    paper = null;
    fulltext = null;
    fullShown = 150_000;
    tab = at !== null ? 'text' : 'overview';
    editingNotes = false;
    allAuthors = false;
    load();
  });

  $effect(() => {
    if (tab === 'text' && fulltext === null && paper?.text) api.fulltext(id).then((t) => (fulltext = t)).catch((e) => toasts.error(e));
  });

  // The text with a marker at the start of the paragraph holding `at`.
  const shownText = $derived.by(() => {
    if (fulltext === null) return '';
    if (at === null || at >= fulltext.length) return fulltext.slice(0, fullShown);
    const start = Math.max(0, fulltext.lastIndexOf('\n\n', at));
    const end = Math.max(fullShown, at + 20_000);
    return `${fulltext.slice(0, start)}\n\n<div id="carrel-hit"></div>\n\n${fulltext.slice(start, end)}`;
  });

  $effect(() => {
    if (tab !== 'text' || at === null || !shownText) return;
    tick().then(() => {
      const hit = document.getElementById('carrel-hit')?.nextElementSibling as HTMLElement | null;
      if (!hit) return;
      hit.scrollIntoView({ block: 'center' });
      hit.classList.add('carrel-hit');
    });
  });

  async function act(name: string, work: () => Promise<unknown>, done?: string) {
    busy = name;
    try {
      const result = (await work()) as { messages?: string[] } | Paper | undefined;
      if (done) toasts.show('success', done, (result as { messages?: string[] })?.messages?.join('\n') || undefined);
      await load();
      app.changed();
    } catch (e) {
      toasts.error(e);
    }
    busy = null;
  }

  const decide = (field: FieldName, approve: boolean) =>
    act(`draft-${field}`, () => api.decide(id, field, approve), approve ? `Approved the ${field}` : `Rejected the ${field}`);

  async function saveNotes() {
    await act('notes', () => api.notes(id, notes), 'Notes saved');
    editingNotes = false;
  }

  function addKeyword(e: SubmitEvent) {
    e.preventDefault();
    const k = keywordInput.trim();
    if (!k || !paper) return;
    keywordInput = '';
    act('keywords', () => api.keywords(id, [...paper!.keywords, k]));
  }

  function addProject(e: SubmitEvent) {
    e.preventDefault();
    const name = projectInput.trim();
    if (!name) return;
    projectInput = '';
    act('project', () => api.toProject(id, name, true), `In ${name}`);
  }

  async function remove() {
    try {
      await api.remove(id);
      toasts.show('success', `Deleted ${id}`);
      app.changed();
      go('/');
    } catch (e) {
      toasts.error(e);
    }
  }

  function attachPicked() {
    const file = pdfPicker.files?.[0];
    pdfPicker.value = '';
    if (file) act('attach', () => api.attach(id, file), 'PDF attached');
  }

  function suppPicked() {
    const files = [...(suppPicker.files ?? [])];
    suppPicker.value = '';
    for (const f of files) act('supp', () => api.supplementary(id, f), `Added ${f.name}`);
  }

  function copyBib() {
    navigator.clipboard.writeText(paper!.bibtex);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  const figure = (src: string) => api.fileUrl(id, 'figures', src.replace(/^figures\//, ''));
  const drafts = $derived(paper ? (Object.entries(paper.drafts) as [FieldName, string][]) : []);
  const tabs = $derived([
    { key: 'overview', label: 'Overview', icon: BookOpen },
    { key: 'notes', label: 'Notes', icon: NotebookPen },
    { key: 'text', label: 'Text', icon: ScanText, hidden: !paper?.text },
    { key: 'figures', label: 'Figures', icon: Image, count: paper?.figures.length, hidden: !paper?.figures.length },
    { key: 'references', label: 'References', icon: Quote },
    { key: 'cited', label: 'Cited by', icon: Link },
    { key: 'connections', label: 'Connections', icon: Network },
    { key: 'bibtex', label: 'BibTeX', icon: FileText },
  ] as const);
</script>

{#if error}
  <div class="mx-auto max-w-xl px-6 py-20 text-center">
    <div class="font-medium">Cannot open {id}</div>
    <p class="mt-1 text-sm text-muted">{error}</p>
    <a class="mt-4 inline-block text-sm text-accent hover:underline" href="#/">Back to the library</a>
  </div>
{:else if !paper}
  <div class="flex h-full items-center justify-center"><Spinner class="size-6 text-faint" /></div>
{:else}
  {@const p = paper}
  {#if reading}
    <!-- Reading: the PDF takes the whole area next to the sidebar. -->
    <div class="flex h-full flex-col">
      <div class="flex h-12 shrink-0 items-center gap-2 border-b bg-card/60 px-3">
        <a href={paperHref(id)} class="flex shrink-0 items-center gap-1.5 rounded-lg px-2 py-1.5 text-sm text-muted hover:bg-hover hover:text-foreground">
          <ArrowLeft class="size-4" />Details
        </a>
        <div class="min-w-0 flex-1 truncate text-sm">
          <span class="font-medium">{p.title}</span>
          <span class="text-faint">· {byline(p)}</span>
        </div>
        <Button size="sm" variant={notesOpen ? 'outline' : 'ghost'} onclick={() => (notesOpen = !notesOpen)} title="Notes beside the PDF">
          <NotebookPen class="size-3.5" />Notes
        </Button>
        {#if p.pdf}
          <a href={api.pdfUrl(id)} target="_blank" rel="noreferrer" title="Open in a new tab" class="rounded-lg p-2 text-muted hover:bg-hover hover:text-foreground"><ExternalLink class="size-4" /></a>
        {/if}
      </div>
      <div class="flex min-h-0 flex-1">
        {#if p.pdf}
          <iframe title="PDF of {p.title}" src={api.pdfUrl(id)} class="min-w-0 flex-1 bg-hover/40"></iframe>
        {:else}
          <div class="flex flex-1 flex-col items-center justify-center gap-4 p-10 text-center">
            <FileUp class="size-10 text-faint" />
            <div class="font-medium">No PDF yet</div>
            <Button variant="primary" busy={busy === 'attach'} onclick={() => pdfPicker.click()}><Paperclip class="size-4" />Attach PDF</Button>
          </div>
        {/if}
        {#if notesOpen}
          <aside class="flex w-80 shrink-0 flex-col gap-3 border-l bg-card/40 p-4 2xl:w-96">
            <div class="text-xs font-semibold tracking-wide text-faint uppercase">Notes</div>
            <textarea
              bind:value={notes}
              placeholder="Your notes, in Markdown."
              class="min-h-0 flex-1 resize-none rounded-xl border bg-card p-3 font-mono text-sm leading-relaxed outline-none placeholder:text-faint focus:border-accent focus:ring-2 focus:ring-accent/20"
              onkeydown={(e) => (e.metaKey || e.ctrlKey) && e.key === 's' && (e.preventDefault(), saveNotes())}
            ></textarea>
            <div class="flex items-center justify-between">
              <span class="text-xs text-faint">Ctrl+S saves</span>
              <Button size="sm" variant="primary" busy={busy === 'notes'} onclick={saveNotes} disabled={notes === (p.notes ?? '')}>Save</Button>
            </div>
          </aside>
        {/if}
      </div>
    </div>
  {:else}
    <div class="flex h-full flex-col">
      <header class="border-b px-6 pt-5 lg:px-10">
        <div class="mx-auto max-w-5xl">
          <div class="flex items-center gap-2 text-sm text-muted">
            <button class="flex cursor-pointer items-center gap-1 hover:text-foreground" onclick={() => history.back()}><ArrowLeft class="size-4" />Back</button>
            <span class="text-faint">/</span>
            <span class="font-mono text-xs">{p.id}</span>
            <div class="ml-1 flex gap-1"><Status pdf={p.pdf} text={p.text} drafts={Object.keys(p.drafts)} /></div>
          </div>
          <div class="mt-3 flex flex-wrap items-start gap-x-8 gap-y-4">
            <div class="min-w-0 flex-1 basis-96">
              <h1 class="text-2xl leading-snug font-semibold tracking-tight">{p.title}</h1>
              <p class="mt-2 text-sm text-muted">
                {allAuthors || p.authors.length <= 12 ? p.authors.join(', ') : `${p.authors.slice(0, 10).join(', ')}, `}
                {#if !allAuthors && p.authors.length > 12}<button class="cursor-pointer text-accent hover:underline" onclick={() => (allAuthors = true)}>and {p.authors.length - 10} more</button>{/if}
              </p>
              <p class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-muted">
                {#if p.venue}<span class="italic">{p.venue}</span>{/if}
                {#if p.year}<span>{p.year}</span>{/if}
                {#if p.doi}<a class="flex items-center gap-1 text-accent hover:underline" href="https://doi.org/{p.doi}" target="_blank" rel="noreferrer">doi:{p.doi}<ExternalLink class="size-3" /></a>{/if}
                {#if p.arxiv}<a class="flex items-center gap-1 text-accent hover:underline" href="https://arxiv.org/abs/{p.arxiv}" target="_blank" rel="noreferrer">arXiv:{p.arxiv}<ExternalLink class="size-3" /></a>{/if}
              </p>
            </div>
            <div class="flex shrink-0 items-center gap-1">
              {#if p.pdf}
                <a href="{paperHref(id)}/read"><Button variant="primary"><BookOpen class="size-4" />Read</Button></a>
              {:else}
                <Button variant="primary" busy={busy === 'attach'} onclick={() => pdfPicker.click()}><Paperclip class="size-4" />Attach PDF</Button>
              {/if}
              <Button variant="ghost" size="icon" busy={busy === 'update'} onclick={() => act('update', () => api.update(id), 'Metadata updated')} title="Fetch the metadata again"><RefreshCw class="size-4" /></Button>
              {#if p.pdf}
                <Button variant="ghost" size="icon" busy={busy === 'extract'} onclick={() => act('extract', () => api.extract(id), 'Text and figures extracted')} title="Extract the text and figures again"><ScanText class="size-4" /></Button>
              {/if}
              <Button variant="ghost" size="icon" onclick={() => (removing = true)} title="Delete"><Trash2 class="size-4 text-failed" /></Button>
            </div>
          </div>
          <nav class="mt-4 flex gap-1 overflow-x-auto [scrollbar-width:none]">
            {#each tabs as t}
              {#if !('hidden' in t && t.hidden)}
                <button
                  class="flex cursor-pointer items-center gap-1.5 border-b-2 px-3 pt-1.5 pb-2.5 text-sm whitespace-nowrap transition {tab === t.key ? 'border-accent font-medium text-foreground' : 'border-transparent text-muted hover:text-foreground'}"
                  onclick={() => (tab = t.key)}
                >
                  <t.icon class="size-4" />{t.label}
                  {#if 'count' in t && t.count}<span class="text-xs text-faint">{t.count}</span>{/if}
                </button>
              {/if}
            {/each}
          </nav>
        </div>
      </header>

      <div class="min-h-0 flex-1 overflow-y-auto">
        <div class="mx-auto max-w-5xl px-6 py-6 lg:px-10">
          {#if tab === 'overview'}
            <div class="grid gap-x-12 gap-y-8 lg:grid-cols-[minmax(0,1fr)_15rem]">
            <div class="min-w-0 space-y-6">
              {#if !p.pdf}
                <div
                  class="flex flex-wrap items-center gap-3 rounded-xl border border-dashed border-wanted/60 bg-wanted/10 px-4 py-3 text-sm"
                  role="region"
                  data-own-drop
                  ondragover={(e) => e.preventDefault()}
                  ondrop={(e) => {
                    e.preventDefault();
                    const file = e.dataTransfer?.files[0];
                    if (file) act('attach', () => api.attach(id, file), 'PDF attached');
                  }}
                >
                  <FileUp class="size-5 text-wanted" />
                  <span class="flex-1">No PDF yet. Drop it here, or:</span>
                  <Button size="sm" busy={busy === 'attach'} onclick={() => pdfPicker.click()}><Paperclip class="size-3.5" />Choose file</Button>
                  {#if p.arxiv}<Button size="sm" busy={busy === 'arxiv'} onclick={() => act('arxiv', () => api.attach(id), 'Downloaded from arXiv')}><Download class="size-3.5" />From arXiv</Button>{/if}
                </div>
              {/if}

              {#each drafts as [field, draft]}
                <div class="overflow-hidden rounded-xl border border-warning/40">
                  <div class="flex items-center gap-2 border-b border-warning/30 bg-warning/10 px-4 py-2.5">
                    <NotebookPen class="size-4 text-warning" />
                    <span class="flex-1 text-sm font-medium">An AI proposes {field === 'keywords' ? 'keywords' : `a ${field === 'references' ? 'list of references' : field}`}</span>
                    <Button size="sm" variant="ghost" busy={busy === `draft-${field}`} onclick={() => decide(field, false)}><X class="size-3.5" />Reject</Button>
                    <Button size="sm" variant="primary" busy={busy === `draft-${field}`} onclick={() => decide(field, true)}><Check class="size-3.5" />Approve</Button>
                  </div>
                  <div class="max-h-96 overflow-y-auto px-4 py-3">
                    {#if field === 'keywords'}
                      <div class="flex flex-wrap gap-1.5">{#each draft.split('\n').filter(Boolean) as k}<Badge tone="warning">{k}</Badge>{/each}</div>
                    {:else if field === 'references'}
                      <pre class="text-xs whitespace-pre-wrap text-muted">{draft}</pre>
                    {:else}
                      <div class="prose">{@html markdown(draft)}</div>
                    {/if}
                  </div>
                </div>
              {/each}

              {#if p.summary}
                <section>
                  <h2 class="mb-2 text-xs font-semibold tracking-wide text-faint uppercase">Summary</h2>
                  <div class="prose">{@html markdown(p.summary)}</div>
                </section>
              {/if}

              {#if p.abstract}
                <section>
                  <h2 class="mb-2 text-xs font-semibold tracking-wide text-faint uppercase">Abstract</h2>
                  <div class="prose text-muted">{@html markdown(p.abstract)}</div>
                </section>
              {/if}

              {#if !p.summary && !p.abstract && !drafts.length}
                <p class="text-sm text-faint">No summary or abstract yet. An AI can propose a summary over MCP; it waits here for you.</p>
              {/if}
            </div>
            <aside class="space-y-6 text-sm">
              <section>
                <h2 class="mb-2 text-xs font-semibold tracking-wide text-faint uppercase">Keywords</h2>
                <div class="flex flex-wrap items-center gap-1.5">
                  {#each p.keywords as k}
                    <span class="group inline-flex items-center gap-1 rounded-md bg-hover px-2 py-0.5 text-xs">
                      <a href="#/?q={encodeURIComponent(`keywords:"${k}"`)}" class="hover:text-accent">{k}</a>
                      <button class="hidden cursor-pointer text-faint group-hover:block hover:text-failed" onclick={() => act('keywords', () => api.keywords(id, p.keywords.filter((x) => x !== k)))} aria-label="Remove {k}"><X class="size-3" /></button>
                    </span>
                  {/each}
                  <form onsubmit={addKeyword}>
                    <input bind:value={keywordInput} placeholder="+ keyword" class="h-6 w-28 rounded-md border border-dashed bg-transparent px-2 text-xs outline-none placeholder:text-faint focus:border-accent" />
                  </form>
                </div>
              </section>

              <section>
                <h2 class="mb-2 text-xs font-semibold tracking-wide text-faint uppercase">Projects</h2>
                <div class="flex flex-wrap items-center gap-1.5">
                  {#each p.projects as name}
                    <span class="group inline-flex items-center gap-1 rounded-md bg-accent/10 px-2 py-0.5 text-xs text-accent">
                      <Folder class="size-3" /><a href="#/?project={encodeURIComponent(name)}" class="hover:underline">{name}</a>
                      <button class="hidden cursor-pointer group-hover:block hover:text-failed" onclick={() => act('project', () => api.toProject(id, name, false))} aria-label="Take out of {name}"><X class="size-3" /></button>
                    </span>
                  {/each}
                  <form onsubmit={addProject} class="flex items-center">
                    <FolderPlus class="mr-1 size-3.5 text-faint" />
                    <input bind:value={projectInput} list="projects" placeholder="add to project" class="h-6 w-32 rounded-md border border-dashed bg-transparent px-2 text-xs outline-none placeholder:text-faint focus:border-accent" />
                    <datalist id="projects">{#each app.projects.filter((x) => !p.projects.includes(x.name)) as x}<option value={x.name}></option>{/each}</datalist>
                  </form>
                </div>
              </section>

              <section>
                <div class="mb-2 flex items-center justify-between">
                  <h2 class="text-xs font-semibold tracking-wide text-faint uppercase">Supplementary files</h2>
                  <Button size="sm" variant="ghost" busy={busy === 'supp'} onclick={() => suppPicker.click()}><Paperclip class="size-3.5" />Add</Button>
                </div>
                {#if p.supplementary.length}
                  <ul class="space-y-1 text-sm">
                    {#each p.supplementary as f}
                      <li><a class="flex items-center gap-2 text-accent hover:underline" href={api.fileUrl(id, 'supplementary', f)} target="_blank" rel="noreferrer"><FileText class="size-3.5" />{f}</a></li>
                    {/each}
                  </ul>
                {:else}
                  <p class="text-sm text-faint">None. Data, code or appendices go here.</p>
                {/if}
              </section>
            </aside>
            </div>
          {:else if tab === 'notes'}
            {#if editingNotes || !p.notes}
              <div class="flex h-full flex-col gap-3">
                <textarea
                  bind:value={notes}
                  placeholder="Your notes, in Markdown. Only you write here; an AI's ideas arrive as drafts."
                  class="min-h-80 flex-1 resize-none rounded-xl border bg-card p-4 font-mono text-sm leading-relaxed outline-none placeholder:text-faint focus:border-accent focus:ring-2 focus:ring-accent/20"
                  onkeydown={(e) => (e.metaKey || e.ctrlKey) && e.key === 's' && (e.preventDefault(), saveNotes())}
                ></textarea>
                <div class="flex items-center justify-end gap-2">
                  <span class="mr-auto text-xs text-faint">Ctrl+S saves</span>
                  {#if p.notes}<Button variant="ghost" onclick={() => ((notes = p.notes ?? ''), (editingNotes = false))}>Cancel</Button>{/if}
                  <Button variant="primary" busy={busy === 'notes'} onclick={saveNotes} disabled={notes === (p.notes ?? '')}>Save notes</Button>
                </div>
              </div>
            {:else}
              <div class="mb-3 flex justify-end"><Button size="sm" onclick={() => (editingNotes = true)}><NotebookPen class="size-3.5" />Edit</Button></div>
              <div class="prose">{@html markdown(p.notes)}</div>
            {/if}
          {:else if tab === 'text'}
            {#if fulltext === null}
              <div class="flex justify-center py-10"><Spinner class="size-6 text-faint" /></div>
            {:else}
              <div class="prose mx-auto max-w-3xl">{@html markdown(shownText, figure)}</div>
              {@const end = Math.max(fullShown, at === null ? 0 : at + 20_000)}
              {#if fulltext.length > end}
                <div class="mt-6 flex justify-center"><Button onclick={() => (fullShown = end + 300_000)}>Show more ({Math.round((fulltext.length - end) / 1000)}k characters left)</Button></div>
              {/if}
            {/if}
          {:else if tab === 'figures'}
            <div class="grid grid-cols-2 gap-4 md:grid-cols-3 xl:grid-cols-4">
              {#each p.figures as f}
                <a href={api.fileUrl(id, 'figures', f)} target="_blank" rel="noreferrer" class="group overflow-hidden rounded-xl border bg-white">
                  <img src={api.fileUrl(id, 'figures', f)} alt={f} loading="lazy" class="aspect-[4/3] w-full object-contain p-2 transition group-hover:scale-[1.02]" />
                  <div class="truncate border-t bg-card px-2 py-1 font-mono text-[0.65rem] text-faint">{f}</div>
                </a>
              {/each}
            </div>
          {:else if tab === 'references'}
            <References {id} />
          {:else if tab === 'cited'}
            <References {id} citedBy />
          {:else if tab === 'connections'}
            <Connections {id} />
          {:else if tab === 'bibtex'}
            <div class="relative">
              <Button size="sm" class="absolute top-2 right-2" onclick={copyBib}>
                {#if copied}<Check class="size-3.5 text-ready" />Copied{:else}<Copy class="size-3.5" />Copy{/if}
              </Button>
              <pre class="overflow-x-auto rounded-xl border bg-card p-4 font-mono text-xs leading-relaxed">{p.bibtex}</pre>
            </div>
          {/if}
        </div>
      </div>
    </div>
  {/if}

  <Dialog bind:open={removing} title="Delete {p.id}?">
    <p class="text-sm text-muted">This deletes the paper and all its files: PDF, text, figures and notes. Other machines move their copy to <code>.carrel/trash</code> at their next sync.</p>
    {#snippet footer()}
      <Button variant="ghost" onclick={() => (removing = false)}>Cancel</Button>
      <Button variant="danger" onclick={remove}><Trash2 class="size-4" />Delete</Button>
    {/snippet}
  </Dialog>
{/if}

<input bind:this={pdfPicker} type="file" accept="application/pdf,.pdf" class="hidden" onchange={attachPicked} />
<input bind:this={suppPicker} type="file" multiple class="hidden" onchange={suppPicked} />
