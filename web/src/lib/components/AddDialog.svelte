<script lang="ts">
  import { FileUp } from '@lucide/svelte';
  import { addSource, uploadPdfs } from '../actions';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  let { open = $bindable(false) }: { open: boolean } = $props();
  let source = $state('');
  let busy = $state(false);
  let picker: HTMLInputElement;

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!source.trim()) return;
    busy = true;
    const id = await addSource(source.trim());
    busy = false;
    if (id) {
      source = '';
      open = false;
    }
  }

  function picked() {
    const files = [...(picker.files ?? [])];
    picker.value = '';
    open = false;
    uploadPdfs(files);
  }
</script>

<Dialog bind:open title="Add a paper">
  <form onsubmit={add} class="space-y-3">
    <label class="block text-sm font-medium" for="source">DOI, arXiv id, URL or exact title</label>
    <input
      id="source"
      bind:value={source}
      placeholder="10.1038/nature14539 · 1706.03762 · Attention is all you need"
      class="h-10 w-full rounded-lg border bg-background px-3 text-sm outline-none placeholder:text-faint focus:border-accent focus:ring-2 focus:ring-accent/20"
      autocomplete="off"
    />
    <p class="text-xs text-muted">carrel looks up the metadata; arXiv papers come with their PDF.</p>
    <div class="flex justify-end">
      <Button variant="primary" type="submit" {busy}>Add</Button>
    </div>
  </form>
  <div class="my-4 flex items-center gap-3 text-xs text-faint"><div class="h-px flex-1 bg-border"></div>or<div class="h-px flex-1 bg-border"></div></div>
  <button
    class="flex w-full cursor-pointer flex-col items-center gap-2 rounded-xl border-2 border-dashed px-4 py-6 text-sm text-muted transition hover:border-accent hover:text-foreground"
    onclick={() => picker.click()}
  >
    <FileUp class="size-6" />
    <span>Choose PDFs, or drop them anywhere on the page</span>
  </button>
  <input bind:this={picker} type="file" accept="application/pdf,.pdf" multiple class="hidden" onchange={picked} />
</Dialog>
