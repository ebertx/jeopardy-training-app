<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import CountdownTimer from '$lib/components/CountdownTimer.svelte';
  import AnswerSheet, { type Sheet } from '$lib/components/AnswerSheet.svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let card = $state<{
    answerId: number;
    answerNorm: string;
    kind: 'answer' | 'fact';
    parent: string | null;
    phrases: Array<{ text: string; tier: string }>;
    category: string;
  } | null>(null);
  let isNew = $state(false);
  let dueCount = $state(0);
  let newRemaining = $state(0);
  let done = $state(false);
  let nextDueAt = $state<string | null>(null);
  let dueSoonCount = $state(0);
  let moreNewAvailable = $state(false);
  let extraMode = $state(false); // past-allowance drilling; resets on reload
  let result = $state<{
    answer: string;
    answerNorm: string;
    kind: 'answer' | 'fact';
    parent: string | null;
    examples: Array<{ clue: string; category: string | null; airDate: string | null }>;
  } | null>(null);
  let loading = $state(true);
  let submitting = $state(false);
  let error = $state('');
  let session = $state({ total: 0, correct: 0 });

  // Answer sheet: shown on demand (e) after reveal, and automatically on a
  // Wrong for a real answer card (paused until Next).
  let sheet = $state<Sheet | null>(null);
  let sheetLoading = $state(false);
  let sheetOpen = $state(false);
  let paused = $state(false);
  let addedNow = $state(0);

  async function fetchNext() {
    loading = true;
    error = '';
    result = null;
    sheet = null;
    sheetOpen = false;
    paused = false;
    addedNow = 0;
    try {
      const res = await api.get(`/api/pavlov/drill/next${extraMode ? '?extra=true' : ''}`);
      dueCount = res.dueCount ?? 0;
      newRemaining = res.newRemaining ?? 0;
      if (res.done) {
        done = true;
        card = null;
        nextDueAt = res.nextDueAt ?? null;
        dueSoonCount = res.dueSoonCount ?? 0;
        moreNewAvailable = res.moreNewAvailable ?? false;
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

  async function fetchSheet() {
    if (!card || card.kind !== 'answer' || sheet || sheetLoading) return;
    sheetLoading = true;
    try {
      sheet = await api.get(`/api/sheet/answer/${encodeURIComponent(card.answerNorm)}`);
    } catch {
      sheet = null; // no key / rejected sheet: nothing to show
    } finally {
      sheetLoading = false;
    }
  }

  function toggleSheet() {
    if (paused || !result || !card || card.kind !== 'answer') return;
    sheetOpen = !sheetOpen;
    if (sheetOpen) fetchSheet();
  }

  async function addFacts() {
    if (!card) return;
    try {
      const res = await api.post('/api/pavlov/facts', { answerNorm: card.answerNorm });
      addedNow = res.added ?? 0;
      if (sheet) sheet = { ...sheet, factsAdded: true };
    } catch (e: any) {
      error = e.message || 'Could not add fact cards';
    }
  }

  async function grade(rating: 'wrong' | 'got_it' | 'too_easy') {
    if (!card || submitting || paused) return;
    submitting = true;
    try {
      const res = await api.post('/api/pavlov/drill/grade', { answerId: card.answerId, rating });
      session = {
        total: session.total + 1,
        correct: session.correct + (rating === 'wrong' ? 0 : 1),
      };
      if (rating === 'wrong' && card.kind === 'answer') {
        // Teaching pause: stay on the card with the sheet open.
        paused = true;
        sheetOpen = true;
        addedNow = res.factsAdded ?? 0;
        await fetchSheet();
        if (addedNow > 0 && sheet) sheet = { ...sheet, factsAdded: true };
      } else {
        await fetchNext();
      }
    } catch (e: any) {
      error = e.message || 'Grade failed';
    } finally {
      submitting = false;
    }
  }

  async function advance() {
    if (!paused || submitting) return;
    submitting = true;
    try {
      await fetchNext();
    } finally {
      submitting = false;
    }
  }

  async function banish() {
    if (!card || submitting) return;
    submitting = true;
    try {
      await api.post(`/api/pavlov/answers/${card.answerId}/suspend`, { suspended: true });
      await fetchNext();
    } catch (e: any) {
      error = e.message || 'Banish failed';
    } finally {
      submitting = false;
    }
  }

  // Space/Enter reveals (or advances while paused); 1/2/3 self-grade after
  // reveal (honesty mode); b banishes anytime; e toggles the sheet; d adds facts.
  function onKeydown(e: KeyboardEvent) {
    if (!card || submitting || loading) return;
    if (e.key === 'b' || e.key === 'B') {
      e.preventDefault();
      banish();
    } else if (paused && (e.key === ' ' || e.key === 'Enter')) {
      e.preventDefault();
      advance();
    } else if (!result && (e.key === ' ' || e.key === 'Enter')) {
      e.preventDefault();
      reveal();
    } else if (result && (e.key === 'e' || e.key === 'E')) {
      e.preventDefault();
      toggleSheet();
    } else if (result && (e.key === 'd' || e.key === 'D')) {
      e.preventDefault();
      if (sheet && !sheet.factsAdded) addFacts();
    } else if (result && !paused) {
      if (e.key === '1') grade('wrong');
      else if (e.key === '2') grade('got_it');
      else if (e.key === '3') grade('too_easy');
    }
  }

  function keepGoing() {
    extraMode = true;
    fetchNext();
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
        Due: {dueCount} · New left: {newRemaining}{#if extraMode}&nbsp;· extra mode{/if}
        {#if session.total > 0}
          · Session: {session.correct}/{session.total}
        {/if}
      </div>
    </div>
    <p class="text-sm text-gray-500 mb-6">
      Trigger keywords → answer. Train the reflex, not the clue. Wrong pauses on the answer sheet;
      e opens it any time, d drills its four facts. Banish (b) removes a bad card —
      undo on the list page.
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
        {#if moreNewAvailable}
          <button
            onclick={keepGoing}
            class="mt-4 px-4 py-2 rounded-lg bg-jeopardy-blue text-white font-medium hover:bg-blue-800 transition-colors"
          >
            Keep going — new cards beyond today's limit
          </button>
        {/if}
      </div>
    {:else if card}
      <!-- Mirrors QuestionCard's layout: category header, centered cue, answer
           box, grade grid — with example clues BELOW the buttons so grading
           never requires scrolling. -->
      <div class="flex flex-col bg-jeopardy-blue rounded-2xl shadow-xl overflow-hidden">
        <div class="px-6 pt-4 flex items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <p class="text-xs font-bold uppercase tracking-widest text-white/60">{card.category}</p>
            {#if isNew}<span class="px-2 py-0.5 rounded-full bg-jeopardy-gold text-jeopardy-blue text-xs font-bold uppercase tracking-wide">new</span>{/if}
          </div>
          <div class="flex items-center gap-2">
            <CountdownTimer resetKey={card.answerId} running={!result} />
            <button
              onclick={banish}
              disabled={submitting}
              title="Remove this card from your deck (undo on the list page)"
              class="text-xs text-white/50 hover:text-red-300 border border-white/20 rounded px-2 py-1 disabled:opacity-50 transition-colors"
            >
              Banish
            </button>
          </div>
        </div>

        <!-- Cue phrases (the question); fact cards name their parent answer -->
        <div class="flex flex-col items-center justify-center px-6 py-8 gap-3">
          {#if card.kind === 'fact' && card.parent}
            <p class="text-sm text-white/60">↳ {card.parent}</p>
          {/if}
          <div class="flex flex-wrap gap-2 justify-center">
            {#each card.phrases as phrase}
              <span class="px-4 py-2 rounded-full border text-xl sm:text-2xl font-bold inline-block
                {phrase.tier === 'hint'
                  ? 'border-white/10 text-white/50'
                  : 'border-white/25 text-jeopardy-gold'}">{phrase.text}</span>
            {/each}
          </div>
        </div>

        <div class="px-6 pb-4">
          {#if !result}
            <button
              onclick={reveal}
              disabled={submitting}
              class="w-full py-3 rounded-xl bg-white/10 hover:bg-white/20 text-white font-semibold text-lg transition-colors border border-white/20 disabled:opacity-50"
            >
              Show Answer
            </button>
            <p class="mt-2 text-center text-xs text-white/40">Space / Enter</p>
          {:else}
            <div class="bg-white rounded-xl px-5 py-4 mb-4 text-center">
              {#if result.kind === 'fact' && result.parent}
                <p class="text-xs text-gray-500 mb-1">↳ {result.parent}</p>
              {/if}
              <p class="text-gray-900 font-bold text-xl">{result.answer}</p>
            </div>
            {#if paused}
              <div class="flex flex-col gap-3">
                <AnswerSheet {sheet} loading={sheetLoading} factsAdded={sheet?.factsAdded ?? false} {addedNow} onAddFacts={addFacts} />
                <button
                  onclick={advance}
                  class="w-full py-3 rounded-xl bg-white/10 hover:bg-white/20 border border-white/20 text-white font-semibold text-lg transition-colors"
                >
                  Next →
                </button>
                <p class="text-center text-xs text-white/40">Space / Enter</p>
              </div>
            {:else}
              <div class="grid grid-cols-3 gap-2">
                <button onclick={() => grade('wrong')} disabled={submitting}
                  class="py-3 rounded-xl bg-red-500 hover:bg-red-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors">Wrong</button>
                <button onclick={() => grade('got_it')} disabled={submitting}
                  class="py-3 rounded-xl bg-green-500 hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors">Got it</button>
                <button onclick={() => grade('too_easy')} disabled={submitting}
                  class="py-3 rounded-xl bg-blue-500 hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed text-white font-semibold text-base transition-colors">Too easy</button>
              </div>
              <p class="mt-2 text-center text-xs text-white/40">1 / 2 / 3</p>
              {#if result.kind === 'answer'}
                {#if sheetOpen}
                  <div class="mt-3">
                    {#if !sheetLoading && sheet === null}
                      <p class="text-white/50 text-sm text-center mt-3">No answer sheet for this one.</p>
                    {:else}
                      <AnswerSheet {sheet} loading={sheetLoading} factsAdded={sheet?.factsAdded ?? false} {addedNow} onAddFacts={addFacts} />
                    {/if}
                  </div>
                {:else}
                  <button onclick={toggleSheet} class="mt-3 w-full py-2 rounded-lg bg-white/10 hover:bg-white/20 border border-white/20 text-white/80 text-sm font-medium transition-colors">
                    Learn this answer <span class="opacity-60">(e)</span>
                  </button>
                {/if}
              {/if}
            {/if}
            {#if result.examples.length > 0}
              <div class="mt-4 pt-4 border-t border-white/10 text-sm text-white/80 space-y-2">
                {#each result.examples as ex}
                  <p>"{ex.clue}" <span class="text-white/50">({ex.category}{ex.airDate ? `, ${ex.airDate}` : ''})</span></p>
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>
