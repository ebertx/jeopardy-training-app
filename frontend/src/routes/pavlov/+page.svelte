<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let card = $state<{ cueId: number; cuePhrases: string[]; category: string } | null>(null);
  let isNew = $state(false);
  let dueCount = $state(0);
  let newRemaining = $state(0);
  let done = $state(false);
  let nextDueAt = $state<string | null>(null);
  let dueSoonCount = $state(0);
  let typed = $state('');
  let result = $state<{ correct: boolean; answer: string; examples: any[] } | null>(null);
  let loading = $state(true);
  let submitting = $state(false);
  let error = $state('');
  let session = $state({ total: 0, correct: 0 });

  async function fetchNext() {
    loading = true;
    error = '';
    result = null;
    typed = '';
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

  async function check() {
    if (!card || submitting) return;
    submitting = true;
    error = '';
    try {
      result = await api.post('/api/pavlov/drill/check', { cueId: card.cueId, typed });
      session = { total: session.total + 1, correct: session.correct + (result!.correct ? 1 : 0) };
    } catch (e: any) {
      error = e.message || 'Check failed';
    } finally {
      submitting = false;
    }
  }

  async function grade(rating: string) {
    if (!card || submitting) return;
    submitting = true;
    try {
      await api.post('/api/pavlov/drill/grade', { cueId: card.cueId, rating });
      await fetchNext();
    } catch (e: any) {
      error = e.message || 'Grade failed';
    } finally {
      submitting = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && card && !result) check();
  }

  onMount(fetchNext);
</script>

<svelte:head><title>Pavlov Drill</title></svelte:head>

<div class="max-w-2xl mx-auto px-4 py-8">
  <div class="flex items-center justify-between mb-6">
    <h1 class="text-2xl font-bold">Pavlov Drill</h1>
    <div class="text-sm text-gray-400">
      Due: {dueCount} · New left: {newRemaining}
      {#if session.total > 0}
        · Session: {session.correct}/{session.total}
      {/if}
    </div>
  </div>
  <p class="text-sm text-gray-400 mb-6">
    Trigger keywords → answer. Train the reflex, not the clue.
    <a href="/pavlov/list" class="text-jeopardy-gold hover:underline">Browse the list →</a>
  </p>

  {#if error}
    <div class="mb-4 p-3 rounded bg-red-900/40 text-red-300 text-sm">{error}</div>
  {/if}

  {#if loading}
    <p class="text-gray-400">Loading…</p>
  {:else if done}
    <div class="p-6 rounded-lg border border-gray-700 text-center">
      <p class="text-lg font-medium mb-2">Done for now 🎉</p>
      {#if dueSoonCount > 0}
        <p class="text-sm text-gray-400">{dueSoonCount} card{dueSoonCount === 1 ? '' : 's'} due within the hour.</p>
      {:else if nextDueAt}
        <p class="text-sm text-gray-400">Next card due {new Date(nextDueAt).toLocaleString()}.</p>
      {:else}
        <p class="text-sm text-gray-400">No cards due. Generate or unsuspend cues from the list page.</p>
      {/if}
    </div>
  {:else if card}
    <div class="p-6 rounded-lg border border-gray-700">
      <div class="flex items-center gap-2 mb-4">
        <span class="px-2 py-0.5 rounded text-xs font-medium bg-jeopardy-gold/20 text-jeopardy-gold">
          {card.category}
        </span>
        {#if isNew}<span class="px-2 py-0.5 rounded text-xs bg-blue-900/50 text-blue-300">new</span>{/if}
      </div>

      <div class="flex flex-wrap gap-2 mb-6">
        {#each card.cuePhrases as phrase}
          <span class="px-3 py-1.5 rounded-full border border-gray-600 text-lg">{phrase}</span>
        {/each}
      </div>

      {#if !result}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          bind:value={typed}
          onkeydown={onKeydown}
          autofocus
          placeholder="Who/what is…?"
          class="w-full px-3 py-2 rounded bg-gray-800 border border-gray-600 focus:border-jeopardy-gold outline-none"
        />
        <button
          onclick={check}
          disabled={submitting}
          class="mt-3 px-4 py-2 rounded bg-jeopardy-gold text-black font-medium disabled:opacity-50"
        >
          Check
        </button>
      {:else}
        <div class="mb-4 p-3 rounded {result.correct ? 'bg-green-900/40 text-green-300' : 'bg-red-900/40 text-red-300'}">
          {result.correct ? 'Correct:' : 'Answer:'} <span class="font-semibold">{result.answer}</span>
        </div>
        {#if result.examples.length > 0}
          <div class="mb-4 text-sm text-gray-400 space-y-2">
            {#each result.examples as ex}
              <p>"{ex.clue}" <span class="text-gray-500">({ex.category}{ex.airDate ? `, ${ex.airDate}` : ''})</span></p>
            {/each}
          </div>
        {/if}
        <div class="flex gap-2">
          {#if result.correct}
            <button onclick={() => grade('got_it')} disabled={submitting}
              class="px-4 py-2 rounded bg-green-700 font-medium disabled:opacity-50">Got it</button>
            <button onclick={() => grade('too_easy')} disabled={submitting}
              class="px-4 py-2 rounded bg-gray-700 font-medium disabled:opacity-50">Too easy</button>
          {:else}
            <button onclick={() => grade('wrong')} disabled={submitting}
              class="px-4 py-2 rounded bg-red-700 font-medium disabled:opacity-50">Continue</button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>
