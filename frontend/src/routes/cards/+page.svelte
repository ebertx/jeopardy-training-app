<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { api } from '$lib/api';
  import CategoryFilter from '$lib/components/CategoryFilter.svelte';
  import Modal from '$lib/components/Modal.svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  type StateFilter = 'learning' | 'due' | 'mastered' | 'struggling';
  const FILTERS: Array<{ key: StateFilter; label: string }> = [
    { key: 'learning', label: 'Learning' },
    { key: 'due', label: 'Due soon' },
    { key: 'mastered', label: 'Mastered' },
    { key: 'struggling', label: 'Struggling' },
  ];

  interface Card {
    id: number;
    question: string | null; // expected response
    answer: string | null; // clue text shown to the player
    category: string | null;
    classifier_category: string | null;
    clue_value: number | null;
    round: number | null;
    air_date: string | null;
    state: string;
    interval_days: number;
    due: string;
    lapses: number;
    suspended: boolean;
  }

  const initial = page.url.searchParams.get('state');
  let stateFilter = $state<StateFilter>(
    initial === 'due' || initial === 'mastered' || initial === 'struggling' ? initial : 'learning'
  );
  let selectedCategory = $state('all');
  let categories = $state<Array<{ name: string; count: number }>>([]);
  let cards = $state<Card[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');
  let expandedId = $state<number | null>(null);
  let resetTarget = $state<Card | null>(null);
  let busyId = $state<number | null>(null);
  let fetchGen = $state(0);

  async function fetchCards() {
    const gen = ++fetchGen;
    loading = true;
    error = '';
    try {
      const params = new URLSearchParams();
      params.set('state', stateFilter);
      if (selectedCategory !== 'all') params.set('category', selectedCategory);
      const res = await api.get(`/api/cards?${params}`);
      if (gen !== fetchGen) return;
      cards = res.cards ?? [];
      total = res.total ?? 0;
    } catch (err: any) {
      if (gen !== fetchGen) return;
      error = err?.message ?? 'Failed to load cards';
    } finally {
      if (gen === fetchGen) loading = false;
    }
  }

  function setFilter(f: StateFilter) {
    stateFilter = f;
    expandedId = null;
    fetchCards();
  }

  function dueLabel(card: Card): string {
    const due = new Date(card.due);
    const now = new Date();
    const ms = due.getTime() - now.getTime();
    if (ms <= 0) return 'due now';
    const hours = ms / 3_600_000;
    if (hours < 24) return `due ${due.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}`;
    return `due in ${Math.round(hours / 24)}d`;
  }

  async function handleReset() {
    if (!resetTarget) return;
    busyId = resetTarget.id;
    try {
      await api.post('/api/mastery/reset', { questionId: resetTarget.id });
      resetTarget = null;
      await fetchCards();
    } catch (err: any) {
      error = err?.message ?? 'Failed to reset card';
    } finally {
      busyId = null;
    }
  }

  async function handleArchive(card: Card) {
    busyId = card.id;
    try {
      await api.post(`/api/questions/${card.id}/archive`, {
        reason: 'Archived from deck browser',
      });
      cards = cards.filter((c) => c.id !== card.id);
      total = Math.max(0, total - 1);
    } catch (err: any) {
      error = err?.message ?? 'Failed to archive';
    } finally {
      busyId = null;
    }
  }

  onMount(async () => {
    try {
      categories = (await api.get('/api/categories')) ?? [];
    } catch {
      // Non-critical
    }
    await fetchCards();
  });
</script>

<svelte:head>
  <title>Cards — Jeopardy! Training</title>
</svelte:head>

<div class="min-h-screen bg-gray-50 py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-4">
    <div class="flex items-center gap-2 flex-wrap">
      <h1 class="text-2xl font-bold text-jeopardy-blue">Cards</h1>
      {#if !loading}
        <span class="text-sm text-gray-500">{total} {total === 1 ? 'card' : 'cards'}</span>
      {/if}
      <button
        onclick={() => goto('/dashboard')}
        class="ml-auto px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
      >
        Done
      </button>
    </div>

    <!-- Filters -->
    <div class="bg-white rounded-xl shadow-sm px-4 py-3 flex flex-col sm:flex-row sm:items-center gap-3">
      <div class="flex gap-1.5 flex-wrap">
        {#each FILTERS as f (f.key)}
          <button
            onclick={() => setFilter(f.key)}
            class="px-3 py-1.5 rounded-full text-sm font-medium transition-colors {stateFilter === f.key
              ? 'bg-jeopardy-blue text-white'
              : 'bg-gray-100 text-gray-600 hover:bg-gray-200'}"
          >
            {f.label}
          </button>
        {/each}
      </div>
      <div class="sm:ml-auto sm:w-56">
        <CategoryFilter
          {categories}
          selected={selectedCategory}
          onchange={(v) => {
            selectedCategory = v;
            fetchCards();
          }}
        />
      </div>
    </div>

    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline text-red-500">Dismiss</button>
      </div>
    {/if}

    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if cards.length === 0}
      <div class="text-center py-16 text-gray-500">No cards match this filter.</div>
    {:else}
      <div class="flex flex-col gap-2">
        {#each cards as card (card.id)}
          {@const expanded = expandedId === card.id}
          <div class="bg-white rounded-xl shadow-sm">
            <button
              class="w-full text-left px-4 py-3"
              onclick={() => (expandedId = expanded ? null : card.id)}
              aria-expanded={expanded}
            >
              <div class="flex items-center gap-2 flex-wrap mb-1">
                <span class="text-xs font-semibold uppercase tracking-wide text-gray-400">
                  {card.classifier_category}
                </span>
                <span class="text-xs px-1.5 py-0.5 rounded-full bg-gray-100 text-gray-600">{card.state}</span>
                <span class="text-xs text-gray-500">{dueLabel(card)}</span>
                {#if card.lapses > 0}
                  <span class="text-xs text-red-500">{card.lapses} lapses</span>
                {/if}
                {#if card.suspended}
                  <span class="text-xs px-1.5 py-0.5 rounded-full bg-red-100 text-red-700 font-semibold">suspended</span>
                {/if}
              </div>
              <p class="text-sm text-gray-800 {expanded ? '' : 'line-clamp-2'}">{card.answer}</p>
            </button>
            {#if expanded}
              <div class="px-4 pb-4 flex flex-col gap-2 border-t border-gray-100 pt-3">
                <p class="text-sm"><span class="font-semibold text-gray-500">Response:</span> <span class="text-gray-900 font-medium">{card.question}</span></p>
                <p class="text-xs text-gray-500">
                  {card.category}
                  {#if card.air_date}&nbsp;· aired {card.air_date}{/if}
                  &nbsp;· interval {Math.round(card.interval_days)}d
                </p>
                <div class="flex gap-2 mt-1">
                  <button
                    onclick={() => (resetTarget = card)}
                    disabled={busyId === card.id}
                    class="px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 disabled:opacity-50 transition-colors"
                  >
                    Reset progress
                  </button>
                  <button
                    onclick={() => handleArchive(card)}
                    disabled={busyId === card.id}
                    class="px-3 py-1.5 rounded-lg border border-red-200 text-sm text-red-600 hover:bg-red-50 disabled:opacity-50 transition-colors"
                  >
                    Archive
                  </button>
                </div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
      {#if total > cards.length}
        <p class="text-center text-xs text-gray-400">Showing first {cards.length} of {total}.</p>
      {/if}
    {/if}
  </div>
</div>

{#if resetTarget}
  <Modal onclose={() => (resetTarget = null)} ariaLabelledby="reset-card-title">
    <div class="rounded-2xl bg-white shadow-2xl p-6 flex flex-col gap-4">
      <h2 id="reset-card-title" class="text-lg font-bold text-gray-800">Reset progress?</h2>
      <p class="text-sm text-gray-600">
        This card returns to the learning queue (interval, lapses, and suspension cleared) and comes due immediately.
      </p>
      <div class="flex gap-3">
        <button
          onclick={() => (resetTarget = null)}
          class="flex-1 py-2.5 rounded-xl border border-gray-300 text-gray-700 text-sm font-medium hover:bg-gray-50 transition-colors"
        >
          Cancel
        </button>
        <button
          onclick={handleReset}
          class="flex-1 py-2.5 rounded-xl bg-red-500 text-white text-sm font-semibold hover:bg-red-600 transition-colors"
        >
          Reset
        </button>
      </div>
    </div>
  </Modal>
{/if}
