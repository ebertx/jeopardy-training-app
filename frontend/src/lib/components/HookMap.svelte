<script lang="ts" module>
  export interface Hook {
    id: number;
    rank: number;
    cue: string;
    support: number;
    source: string;
    seen: number;
    lastWrongAt: string | null;
  }
  export interface Entity {
    answer: string;
    forms: string[];
    hooks: Hook[];
  }
  export interface ExampleClue {
    clue: string;
    category: string | null;
    airDate: string | null;
  }
</script>

<script lang="ts">
  let {
    entity,
    servedHookId = null,
    exampleClue = null,
    dark = true,
    onDrop,
  }: {
    entity: Entity;
    servedHookId?: number | null;
    exampleClue?: ExampleClue | null;
    dark?: boolean;
    onDrop?: (id: number) => void;
  } = $props();

  const muted = $derived(dark ? 'text-white/50' : 'text-gray-400');
  const served = $derived(dark ? 'text-jeopardy-gold font-semibold' : 'text-jeopardy-blue font-semibold');
  const fmt = (iso: string) => new Date(iso).toLocaleDateString([], { month: 'numeric', day: 'numeric' });
</script>

<!-- The entity's hooks: filled dot = seen, hollow = not yet, arrow = served now. -->
<div class="text-left {dark ? 'text-white' : 'text-gray-900'}">
  {#if entity.forms.length > 1}
    <p class="text-xs {muted} mb-1">{entity.forms.join(' · ')}</p>
  {/if}
  {#if entity.hooks.length === 0}
    <p class="text-sm {muted}">No hooks yet for this answer.</p>
  {:else}
    <ul class="space-y-1 text-sm">
      {#each entity.hooks as h (h.id)}
        <li class="flex items-baseline gap-2 {h.id === servedHookId ? served : ''}">
          <span class="w-4 shrink-0 text-center">{h.id === servedHookId ? '▶' : h.seen > 0 ? '●' : '○'}</span>
          <span class="flex-1 min-w-0">{h.cue}</span>
          <span class="text-xs {muted} tabular-nums">{h.support}</span>
          {#if h.lastWrongAt}
            <span class="text-xs {dark ? 'text-red-300' : 'text-red-500'}">✗ {fmt(h.lastWrongAt)}</span>
          {/if}
          {#if onDrop && h.id === servedHookId}
            <button
              onclick={() => onDrop(h.id)}
              title="Drop this hook from the deck (restore on the list page)"
              class="text-xs {muted} hover:text-red-300 border {dark ? 'border-white/20' : 'border-gray-300'} rounded px-1.5"
            >drop <span class="opacity-60">(x)</span></button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if exampleClue}
    <p class="mt-2 text-sm {dark ? 'text-white/80' : 'text-gray-700'}">
      e.g. “{exampleClue.clue}”
      <span class={muted}>({exampleClue.category}{exampleClue.airDate ? `, ${exampleClue.airDate}` : ''})</span>
    </p>
  {/if}
</div>
