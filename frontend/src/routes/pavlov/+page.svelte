<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let card = $state<{
    answerId: number;
    phrases: Array<{ text: string; tier: string }>;
    category: string;
  } | null>(null);
  let isNew = $state(false);
  let dueCount = $state(0);
  let newRemaining = $state(0);
  let done = $state(false);
  let nextDueAt = $state<string | null>(null);
  let dueSoonCount = $state(0);
  let result = $state<{
    answer: string;
    examples: Array<{ clue: string; category: string | null; airDate: string | null }>;
  } | null>(null);
  let loading = $state(true);
  let submitting = $state(false);
  let error = $state('');
  let session = $state({ total: 0, correct: 0 });

  async function fetchNext() {
    loading = true;
    error = '';
    result = null;
    try {
      const res = await api.get('/api/pavlov/drill/next');
      dueCount = res.dueCount ?? 0;
      newRemaining = res.newRemaining ?? 0;
      if (res.done) {
        done = true;
        card = null;
        nextDueAt = res.nextDueAt ?? null;
        dueSoonCount = res.dueSoonCount ?? 0;
      } else {
        done = false;
        card = res.card;
        isNew = res.isNew;
      }
    } catch (e: any) {
      error = e.message || 'Failed to load';
    } finally {
      loading = false;
    }
  }

  async function reveal() {
    if (!card || submitting) return;
    submitting = true;
    error = '';
    try {
      result = await api.post('/api/pavlov/drill/check', { answerId: card.answerId });
    } catch (e: any) {
      error = e.message || 'Reveal failed';
    } finally {
      submitting = false;
    }
  }

  async function grade(rating: 'wrong' | 'got_it' | 'too_easy') {
    if (!card || submitting) return;
    submitting = true;
    try {
      await api.post('/api/pavlov/drill/grade', { answerId: card.answerId, rating });
      session = {
        total: session.total + 1,
        correct: session.correct + (rating === 'wrong' ? 0 : 1),
      };
      await fetchNext();
    } catch (e: any) {
      error = e.message || 'Grade failed';
    } finally {
      submitting = false;
    }
  }

  // Space/Enter reveals; 1/2/3 self-grade after reveal (honesty mode).
  function onKeydown(e: KeyboardEvent) {
    if (!card || submitting) return;
    if (!result && (e.key === ' ' || e.key === 'Enter')) {
      e.preventDefault();
      reveal();
    } else if (result) {
      if (e.key === '1') grade('wrong');
      else if (e.key === '2') grade('got_it');
      else if (e.key === '3') grade('too_easy');
    }
  }

  onMount(fetchNext);
</script>

<svelte:head><title>Pavlov Drill</title></svelte:head>
<svelte:window onkeydown={onKeydown} />

<div class="min-h-screen bg-gray-50 py-6 sm:py-8 px-4">
  <div class="max-w-2xl mx-auto">
    <div class="flex items-center justify-between mb-6">
      <h1 class="text-xl sm:text-2xl font-bold text-jeopardy-blue">Pavlov Drill</h1>
      <div class="text-sm font-medium text-gray-600">
        Due: {dueCount} · New left: {newRemaining}
        {#if session.total > 0}
          · Session: {session.correct}/{session.total}
        {/if}
      </div>
    </div>
    <p class="text-sm text-gray-500 mb-6">
      Trigger keywords → answer. Train the reflex, not the clue.
      <a href="/pavlov/list" class="text-jeopardy-blue hover:underline">Browse the list →</a>
    </p>

    {#if error}
      <div class="mb-4 px-4 py-3 rounded-lg bg-red-50 border border-red-200 text-red-700 text-sm">{error}</div>
    {/if}

    {#if loading}
      <p class="text-gray-500">Loading…</p>
    {:else if done}
      <div class="p-6 rounded-xl border border-gray-200 bg-white shadow-sm text-center">
        <p class="text-lg font-medium mb-2 text-gray-900">Done for now 🎉</p>
        {#if dueSoonCount > 0}
          <p class="text-sm text-gray-500">{dueSoonCount} card{dueSoonCount === 1 ? '' : 's'} due within the hour.</p>
        {:else if nextDueAt}
          <p class="text-sm text-gray-500">Next card due {new Date(nextDueAt).toLocaleString()}.</p>
        {:else}
          <p class="text-sm text-gray-500">No cards due. Generate or unsuspend cues from the list page.</p>
        {/if}
      </div>
    {:else if card}
      <div class="p-6 rounded-xl border border-gray-200 bg-white shadow-sm">
        <div class="flex items-center gap-2 mb-4">
          <span class="px-2 py-0.5 rounded-full text-xs font-medium bg-blue-100 text-blue-800">
            {card.category}
          </span>
          {#if isNew}<span class="px-2 py-0.5 rounded-full bg-jeopardy-gold text-jeopardy-blue text-xs font-bold uppercase tracking-wide">new</span>{/if}
        </div>

        <div class="mb-6 flex flex-wrap gap-2">
          {#each card.phrases as phrase}
            <span class="px-4 py-2 rounded-full border text-xl inline-block
              {phrase.tier === 'hint'
                ? 'border-gray-200 text-gray-500'
                : 'border-gray-300 text-gray-900'}">{phrase.text}</span>
          {/each}
        </div>

        {#if !result}
          <button
            onclick={reveal}
            disabled={submitting}
            class="px-4 py-2 rounded-lg bg-jeopardy-blue text-white font-medium hover:bg-blue-800 transition-colors disabled:opacity-50"
          >
            Show answer
          </button>
          <span class="ml-3 text-xs text-gray-400">Space/Enter</span>
        {:else}
          <div class="mb-4 p-3 rounded-lg bg-blue-50 border border-blue-200 text-gray-900">
            Answer: <span class="font-semibold">{result.answer}</span>
          </div>
          {#if result.examples.length > 0}
            <div class="mb-4 text-sm text-gray-600 space-y-2">
              {#each result.examples as ex}
                <p>"{ex.clue}" <span class="text-gray-500">({ex.category}{ex.airDate ? `, ${ex.airDate}` : ''})</span></p>
              {/each}
            </div>
          {/if}
          <div class="flex gap-2 items-center">
            <button onclick={() => grade('wrong')} disabled={submitting}
              class="px-4 py-2 rounded-xl bg-red-500 hover:bg-red-600 text-white font-semibold disabled:opacity-50 transition-colors">Wrong</button>
            <button onclick={() => grade('got_it')} disabled={submitting}
              class="px-4 py-2 rounded-xl bg-green-500 hover:bg-green-600 text-white font-semibold disabled:opacity-50 transition-colors">Got it</button>
            <button onclick={() => grade('too_easy')} disabled={submitting}
              class="px-4 py-2 rounded-xl bg-blue-500 hover:bg-blue-600 text-white font-semibold disabled:opacity-50 transition-colors">Too easy</button>
            <span class="text-xs text-gray-400">1 / 2 / 3</span>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
