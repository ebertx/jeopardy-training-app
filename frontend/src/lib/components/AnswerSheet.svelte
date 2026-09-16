<script lang="ts" module>
  export interface Sheet {
    answerNorm: string;
    answer: string;
    identity: string;
    facts: Array<{ prompt: string; response: string }>;
    factsAdded: boolean;
  }
</script>

<script lang="ts">
  let {
    sheet,
    loading,
    factsAdded,
    addedNow = 0,
    onAddFacts,
  }: {
    sheet: Sheet | null;
    loading: boolean;
    factsAdded: boolean;
    addedNow?: number;
    onAddFacts: () => Promise<void>;
  } = $props();

  let adding = $state(false);
  async function add() {
    if (adding || factsAdded) return;
    adding = true;
    try {
      await onAddFacts();
    } finally {
      adding = false;
    }
  }
</script>

<!-- Dark-surface renderer (both the Pavlov card and the practice card are blue). -->
{#if loading && !sheet}
  <div class="flex items-center gap-2 text-white/70 text-sm py-2">
    <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-jeopardy-gold"></div>
    Building the answer sheet…
  </div>
{:else if sheet}
  <div class="bg-white/10 border border-white/20 rounded-xl px-4 py-3 text-left">
    <p class="text-xs uppercase tracking-wide text-white/50 mb-1">Answer sheet</p>
    <p class="text-white/90 text-sm leading-relaxed">{sheet.identity}</p>
    <dl class="mt-3 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
      {#each sheet.facts as f (f.prompt)}
        <dt class="text-white/60">{f.prompt}</dt>
        <dd class="text-jeopardy-gold font-semibold">→ {f.response}</dd>
      {/each}
    </dl>
    <div class="mt-3 flex items-center gap-3">
      {#if factsAdded}
        <span class="text-xs text-green-300">{addedNow > 0 ? `${addedNow} fact cards added` : 'In your deck'}</span>
      {:else}
        <button
          onclick={add}
          disabled={adding}
          class="px-3 py-1.5 rounded-lg bg-jeopardy-gold text-jeopardy-blue text-xs font-bold hover:bg-yellow-400 disabled:opacity-50 transition-colors"
        >
          Drill these 4 <span class="font-normal opacity-70">(d)</span>
        </button>
      {/if}
    </div>
  </div>
{/if}
