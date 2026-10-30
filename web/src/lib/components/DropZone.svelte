<script lang="ts">
  import { FileUp } from '@lucide/svelte';
  import { uploadPdfs } from '../actions';

  // PDFs dropped anywhere on the page are added to the library.
  let depth = $state(0);
  const files = (e: DragEvent) => e.dataTransfer?.types.includes('Files');
</script>

<svelte:window
  ondragenter={(e) => files(e) && depth++}
  ondragleave={(e) => files(e) && depth--}
  ondragover={(e) => files(e) && e.preventDefault()}
  ondrop={(e) => {
    if (!files(e) || (e.target as HTMLElement).closest('[data-own-drop]')) return depth = 0;
    e.preventDefault();
    depth = 0;
    uploadPdfs([...(e.dataTransfer?.files ?? [])]);
  }}
/>

{#if depth > 0}
  <div class="pointer-events-none fixed inset-0 z-40 flex items-center justify-center bg-background/80 backdrop-blur-sm">
    <div class="flex flex-col items-center gap-3 rounded-2xl border-2 border-dashed border-accent bg-card px-16 py-12 shadow-xl">
      <FileUp class="size-10 text-accent" />
      <div class="text-lg font-semibold">Drop PDFs to add them</div>
      <div class="text-sm text-muted">carrel identifies each one and extracts its text</div>
    </div>
  </div>
{/if}
