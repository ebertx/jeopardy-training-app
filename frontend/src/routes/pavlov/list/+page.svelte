<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });
  let isAdmin = $derived(auth.user?.role === 'admin');

  type Phrase = { text: string; tier: string; support?: number; total?: number; precision?: number };
  type HookRow = {
    id: number; rank: number; cue: string | null; keyGram: string;
    support: number; source: string; status: 'active' | 'dropped';
  };
  type Card = {
    id: number; answer: string; category: string; forms: string[]; vetted: boolean;
    phrases: Phrase[]; suspended: boolean; hooks: HookRow[];
  };
  type Example = { clue: string; category: string | null; airDate: string | null };

  let cards = $state<Card[]>([]);
  let search = $state('');
  let vettedOnly = $state(false);
  let loading = $state(true);
  let error = $state('');
  let expanded = $state<Set<number>>(new Set());
  let examples = $state<Record<number, Example[]>>({});
  let genStatus = $state<{
    running: boolean; pending: number; active: number; dropped: number;
    hooks?: { total: number; labeled: number; unlabeled: number; vetted: number; entitiesPending: number };
  } | null>(null);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  let filtered = $derived(
    cards.filter((c) => {
      if (vettedOnly && !c.vetted) return false;
      const q = search.trim().toLowerCase();
      if (!q) return true;
      return (
        c.answer.toLowerCase().includes(q) ||
        c.category.toLowerCase().includes(q) ||
        c.forms.some((f) => f.toLowerCase().includes(q)) ||
        c.hooks.some((h) => (h.cue ?? h.keyGram).toLowerCase().includes(q)) ||
        c.phrases.some((p) => p.text.toLowerCase().includes(q))
      );
    })
  );
  let grouped = $derived.by(() => {
    const groups: Array<{ category: string; items: Card[] }> = [];
    for (const c of filtered) {
      const last = groups[groups.length - 1];
      if (last && last.category === c.category) last.items.push(c);
      else groups.push({ category: c.category, items: [c] });
    }
    return groups;
  });

  async function load() {
    loading = true;
    try {
      const res = await api.get('/api/pavlov/answers');
      cards = res.answers ?? [];
    } catch (e: any) {
      error = e.message || 'Failed to load';
    } finally {
      loading = false;
    }
  }

  async function toggleSuspend(card: Card) {
    const next = !card.suspended;
    try {
      await api.post(`/api/pavlov/answers/${card.id}/suspend`, { suspended: next });
      card.suspended = next;
    } catch (e: any) {
      error = e.message || 'Suspend failed';
    }
  }

  async function toggleHook(h: HookRow) {
    const action = h.status === 'active' ? 'drop' : 'restore';
    try {
      const res = await api.post(`/api/pavlov/hooks/${h.id}/${action}`);
      h.status = res.status;
    } catch (e: any) {
      error = e.message || 'Hook update failed';
    }
  }

  async function toggleExpand(card: Card) {
    const next = new Set(expanded);
    if (next.has(card.id)) {
      next.delete(card.id);
      expanded = next;
      return;
    }
    next.add(card.id);
    expanded = next;
    for (const h of card.hooks) {
      if (examples[h.id]) continue;
      try {
        const res = await api.get(`/api/pavlov/hooks/${h.id}`);
        examples = { ...examples, [h.id]: res.examples ?? [] };
      } catch {
        examples = { ...examples, [h.id]: [] };
      }
    }
  }

  async function refreshStatus() {
    try {
      genStatus = await api.get('/api/admin/pavlov/status');
      if (genStatus && !genStatus.running && pollTimer) {
        clearInterval(pollTimer);
        pollTimer = null;
        await load();
      }
    } catch {
      /* non-admin or transient; ignore */
    }
  }

  async function runJob(job: 'generate' | 'resolve' | 'hooks') {
    error = '';
    try {
      await api.post(`/api/admin/pavlov/${job}`);
      await refreshStatus();
      if (!pollTimer) pollTimer = setInterval(refreshStatus, 5000);
    } catch (e: any) {
      error = e.message || `${job} failed`;
    }
  }

  onMount(async () => {
    await load();
    if (isAdmin) await refreshStatus();
  });
  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });
</script>

<svelte:head><title>Pavlov Entities</title></svelte:head>

<div class="min-h-screen bg-gray-50 py-6 sm:py-8 px-4">
  <div class="max-w-4xl mx-auto">
    <div class="flex items-center justify-between mb-2">
      <h1 class="text-xl sm:text-2xl font-bold text-jeopardy-blue">Pavlov Entities</h1>
      <a href="/pavlov" class="text-jeopardy-blue hover:underline text-sm">Drill →</a>
    </div>
    <p class="text-sm text-gray-500 mb-6">
      One row per answer, with the angles (hooks) Jeopardy! writers use for it, mined from its own clues.
      ★ marks answers named on the community Pavlov lists. Expand a row to see each hook's example clues; drop hooks or suspend whole answers you don't want in your drill.
    </p>

    {#if isAdmin}
      <div class="mb-4 p-3 rounded-xl border border-gray-200 bg-white shadow-sm flex flex-wrap items-center gap-3 text-sm">
        <button onclick={() => runJob('resolve')} disabled={genStatus?.running}
          class="px-3 py-1.5 rounded-lg bg-jeopardy-gold text-jeopardy-blue font-medium disabled:opacity-50 hover:bg-yellow-400 transition-colors">Resolve entities</button>
        <button onclick={() => runJob('generate')} disabled={genStatus?.running}
          class="px-3 py-1.5 rounded-lg bg-jeopardy-gold text-jeopardy-blue font-medium disabled:opacity-50 hover:bg-yellow-400 transition-colors">Generate cues</button>
        <button onclick={() => runJob('hooks')} disabled={genStatus?.running}
          class="px-3 py-1.5 rounded-lg bg-jeopardy-gold text-jeopardy-blue font-medium disabled:opacity-50 hover:bg-yellow-400 transition-colors">Build hooks</button>
        {#if genStatus}
          <span class="text-gray-500">
            {genStatus.running ? 'running · ' : ''}cues {genStatus.active} active · {genStatus.pending} pending
            {#if genStatus.hooks}
              · hooks {genStatus.hooks.labeled}/{genStatus.hooks.total} labeled · {genStatus.hooks.vetted} vetted · {genStatus.hooks.entitiesPending} entities pending
            {/if}
          </span>
        {/if}
      </div>
    {/if}

    {#if error}
      <div class="mb-4 px-4 py-3 rounded-lg bg-red-50 border border-red-200 text-red-700 text-sm">{error}</div>
    {/if}

    <div class="flex flex-wrap items-center gap-3 mb-6">
      <input type="text" bind:value={search} placeholder="Search answers, forms, hooks, categories…"
        class="flex-1 min-w-[200px] px-3 py-2 rounded-lg bg-white border border-gray-300 text-gray-900 focus:border-jeopardy-blue focus:outline-none focus:ring-1 focus:ring-jeopardy-blue" />
      <label class="flex items-center gap-2 text-sm text-gray-700 cursor-pointer">
        <input type="checkbox" bind:checked={vettedOnly} /> vetted only
      </label>
    </div>

    {#if loading}
      <p class="text-gray-500">Loading…</p>
    {:else if cards.length === 0}
      <p class="text-gray-500">No entities yet{isAdmin ? ' — run Resolve, Generate, then Build hooks above.' : '.'}</p>
    {:else}
      {#each grouped as group}
        <h2 class="text-lg font-semibold mt-6 mb-2 text-jeopardy-blue">
          {group.category} <span class="text-gray-500 text-sm font-normal">({group.items.length})</span>
        </h2>
        <div class="divide-y divide-gray-200 border border-gray-200 rounded-xl bg-white shadow-sm overflow-hidden">
          {#each group.items as card (card.id)}
            <div class="p-3 {card.suspended ? 'opacity-40' : ''}">
              <div class="flex items-start gap-3">
                <button onclick={() => toggleExpand(card)} class="text-gray-400 hover:text-jeopardy-blue w-5 shrink-0 text-left" title="Show example clues">
                  {expanded.has(card.id) ? '▾' : '▸'}
                </button>
                <div class="flex-1 min-w-0">
                  <div class="text-gray-900">
                    {#if card.vetted}<span class="text-jeopardy-gold mr-1" title="On the community Pavlov lists">★</span>{/if}
                    <span class="font-semibold">{card.answer}</span>
                    {#if card.forms.length > 1}
                      <span class="text-xs text-gray-400 ml-2">{card.forms.filter((f) => f !== card.answer).join(' · ')}</span>
                    {/if}
                  </div>
                  <ul class="mt-1 space-y-0.5 text-sm">
                    {#each card.hooks as h (h.id)}
                      <li class="flex items-baseline gap-2 {h.status === 'dropped' ? 'opacity-40 line-through' : ''}">
                        <span class="text-gray-400 w-4 text-right tabular-nums">{h.rank}</span>
                        {#if h.cue}
                          <span class="flex-1 min-w-0">{h.cue}</span>
                        {:else}
                          <span class="flex-1 min-w-0 text-gray-400 italic">unlabeled · {h.keyGram}</span>
                        {/if}
                        <span class="text-xs text-gray-400">{h.support} · {h.source}</span>
                        <button onclick={() => toggleHook(h)} class="text-xs text-gray-500 hover:text-jeopardy-blue">
                          {h.status === 'active' ? 'drop' : 'restore'}
                        </button>
                      </li>
                      {#if expanded.has(card.id) && examples[h.id]}
                        {#each examples[h.id] as ex}
                          <li class="pl-6 text-xs text-gray-500">“{ex.clue}” ({ex.category}{ex.airDate ? `, ${ex.airDate}` : ''})</li>
                        {/each}
                      {/if}
                    {/each}
                    {#if card.hooks.length === 0 && card.phrases.length > 0}
                      <li class="text-gray-500">
                        {#each card.phrases as phrase, i}
                          {#if i > 0}<span class="text-gray-400 mx-1">·</span>{/if}
                          <span class={phrase.tier === 'hint' ? 'text-gray-400' : ''}>{phrase.text}</span>
                        {/each}
                        <span class="text-xs text-gray-400 ml-1">(legacy cues — no hooks yet)</span>
                      </li>
                    {/if}
                  </ul>
                </div>
                <button onclick={() => toggleSuspend(card)}
                  class="text-xs px-2 py-1 rounded-lg border border-gray-300 hover:border-jeopardy-blue shrink-0 text-gray-700 transition-colors">
                  {card.suspended ? 'Unsuspend' : 'Suspend'}
                </button>
              </div>
            </div>
          {/each}
        </div>
      {/each}
    {/if}
  </div>
</div>
