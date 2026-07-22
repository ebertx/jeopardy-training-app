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

  type Cue = {
    id: number; answer: string; category: string;
    cue: string; support: number; total: number; precision: number; suspended: boolean;
  };
  let cues = $state<Cue[]>([]);
  let search = $state('');
  let loading = $state(true);
  let error = $state('');
  let genStatus = $state<{ running: boolean; pending: number; active: number; dropped: number } | null>(null);
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  // Server returns cues pre-sorted by test-weight category order then support × precision;
  // grouping just walks that order.
  let filtered = $derived(
    cues.filter((c) => {
      const q = search.trim().toLowerCase();
      if (!q) return true;
      return (
        c.answer.toLowerCase().includes(q) ||
        c.category.toLowerCase().includes(q) ||
        c.cue.toLowerCase().includes(q)
      );
    })
  );
  let grouped = $derived.by(() => {
    const groups: Array<{ category: string; items: Cue[] }> = [];
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
      const res = await api.get('/api/pavlov/cues');
      cues = res.cues ?? [];
    } catch (e: any) {
      error = e.message || 'Failed to load cues';
    } finally {
      loading = false;
    }
  }

  async function toggleSuspend(cue: Cue) {
    const next = !cue.suspended;
    try {
      await api.post(`/api/pavlov/cues/${cue.id}/suspend`, { suspended: next });
      cue.suspended = next;
    } catch (e: any) {
      error = e.message || 'Suspend failed';
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

  async function generate() {
    error = '';
    try {
      await api.post('/api/admin/pavlov/generate');
      await refreshStatus();
      if (!pollTimer) pollTimer = setInterval(refreshStatus, 5000);
    } catch (e: any) {
      error = e.message || 'Generate failed';
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

<svelte:head><title>Pavlov Cues</title></svelte:head>

<div class="min-h-screen bg-gray-50 py-6 sm:py-8 px-4">
  <div class="max-w-4xl mx-auto">
    <div class="flex items-center justify-between mb-2">
      <h1 class="text-xl sm:text-2xl font-bold text-jeopardy-blue">Pavlov Cues</h1>
      <a href="/pavlov" class="text-jeopardy-blue hover:underline text-sm">Drill →</a>
    </div>
    <p class="text-sm text-gray-500 mb-6">
      Signature keyword → answer associations mined from the clue corpus — kept only when the phrase recurs across the answer's clues and reliably means that answer. Suspend rows you don't want in your drill deck.
    </p>

    {#if isAdmin}
      <div class="mb-4 p-3 rounded-xl border border-gray-200 bg-white shadow-sm flex items-center gap-4 text-sm">
        <button
          onclick={generate}
          disabled={genStatus?.running}
          class="px-3 py-1.5 rounded-lg bg-jeopardy-gold text-jeopardy-blue font-medium disabled:opacity-50 hover:bg-yellow-400 transition-colors"
        >
          {genStatus?.running ? 'Generating…' : 'Generate / resume'}
        </button>
        {#if genStatus}
          <span class="text-gray-500">
            active {genStatus.active} · pending {genStatus.pending} · dropped {genStatus.dropped}
          </span>
        {/if}
      </div>
    {/if}

    {#if error}
      <div class="mb-4 px-4 py-3 rounded-lg bg-red-50 border border-red-200 text-red-700 text-sm">{error}</div>
    {/if}

    <input
      type="text"
      bind:value={search}
      placeholder="Search cues, answers, categories…"
      class="w-full mb-6 px-3 py-2 rounded-lg bg-white border border-gray-300 text-gray-900 focus:border-jeopardy-blue focus:outline-none focus:ring-1 focus:ring-jeopardy-blue"
    />

    {#if loading}
      <p class="text-gray-500">Loading…</p>
    {:else if cues.length === 0}
      <p class="text-gray-500">No cues yet{isAdmin ? ' — run Generate above.' : '.'}</p>
    {:else}
      {#each grouped as group}
        <h2 class="text-lg font-semibold mt-6 mb-2 text-jeopardy-blue">
          {group.category} <span class="text-gray-500 text-sm font-normal">({group.items.length})</span>
        </h2>
        <div class="divide-y divide-gray-200 border border-gray-200 rounded-xl bg-white shadow-sm overflow-hidden">
          {#each group.items as cue (cue.id)}
            <div class="p-3 flex items-start gap-3 {cue.suspended ? 'opacity-40' : ''}">
              <div class="flex-1 min-w-0">
                <div class="text-gray-900">
                  <span class="font-medium">{cue.cue}</span>
                  <span class="text-gray-400 mx-1">→</span>
                  <span>{cue.answer}</span>
                </div>
                <div class="text-xs text-gray-500 mt-0.5">
                  in {cue.support} of its clues · {Math.round(cue.precision * 100)}% precise corpus-wide ({cue.support}/{cue.total})
                </div>
              </div>
              <button
                onclick={() => toggleSuspend(cue)}
                class="text-xs px-2 py-1 rounded-lg border border-gray-300 hover:border-jeopardy-blue shrink-0 text-gray-700 transition-colors"
              >
                {cue.suspended ? 'Unsuspend' : 'Suspend'}
              </button>
            </div>
          {/each}
        </div>
      {/each}
    {/if}
  </div>
</div>
