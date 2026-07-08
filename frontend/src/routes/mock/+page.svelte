<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { onMount, onDestroy } from 'svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  const CLUE_MS = 15000;

  type Phase = 'idle' | 'active' | 'results';
  let phase = $state<Phase>('idle');
  let loading = $state(false);
  let error = $state('');
  let hasResumable = $state(false);

  // Active-test state
  let testId = $state<number | null>(null);
  let position = $state(0);
  let total = $state(50);
  let clue = $state<{ id: number; category: string; text: string } | null>(null);
  let typed = $state('');
  let deadline = 0;               // performance.now() when the clock hits zero
  let remainingMs = $state(CLUE_MS);
  let timerHandle: ReturnType<typeof setInterval> | null = null;
  let submitting = $state(false);
  let inputEl = $state<HTMLInputElement | null>(null);

  // Results state
  let results = $state<any>(null);
  let overridingPos = $state<number | null>(null);

  onMount(async () => {
    // Detect a resumable test without starting a new one.
    try {
      await api.get('/api/mock-test/current');
      hasResumable = true;
    } catch { /* 404 = none */ }
  });

  function stopTimer() {
    if (timerHandle) clearInterval(timerHandle);
    timerHandle = null;
  }

  function startTimer() {
    stopTimer();
    deadline = performance.now() + CLUE_MS;
    remainingMs = CLUE_MS;
    timerHandle = setInterval(() => {
      remainingMs = Math.max(0, deadline - performance.now());
      if (remainingMs <= 0) submit();   // auto-submit whatever is typed
    }, 100);
  }

  async function loadCurrent() {
    const cur = await api.get('/api/mock-test/current');
    testId = cur.testId;
    position = cur.position;
    total = cur.total;
    clue = cur.clue;
    typed = '';
    phase = 'active';
    startTimer();
    queueMicrotask(() => inputEl?.focus());
  }

  async function start() {
    loading = true;
    error = '';
    try {
      await api.post('/api/mock-test');
      await loadCurrent();
    } catch (e: any) {
      error = e?.message ?? 'Could not start test';
    } finally {
      loading = false;
    }
  }

  async function submit() {
    if (submitting || phase !== 'active') return;
    submitting = true;
    stopTimer();
    const responseMs = Math.min(CLUE_MS, Math.max(0, Math.round(CLUE_MS - (deadline - performance.now()))));
    try {
      const res = await api.post('/api/mock-test/answer', {
        position,
        typedAnswer: typed,
        responseMs,
      });
      if (res.completed) {
        await showResults(testId!);
      } else {
        await loadCurrent();
      }
    } catch (e: any) {
      if (e?.status === 409) {
        await loadCurrent();      // position drift — resync
      } else {
        error = e?.message ?? 'Submit failed';
      }
    } finally {
      submitting = false;
    }
  }

  async function showResults(id: number) {
    results = await api.get(`/api/mock-test/${id}/results`);
    phase = 'results';
  }

  async function toggleOverride(row: any) {
    if (overridingPos !== null) return;
    overridingPos = row.position;
    try {
      const res = await api.post(`/api/mock-test/${testId}/override`, {
        position: row.position,
        correct: !row.finalCorrect,
      });
      row.finalCorrect = !row.finalCorrect;
      row.overridden = true;
      results.score = res.score;
    } finally {
      overridingPos = null;
    }
  }

  let addingMisses = $state(false);
  let missesAdded = $state<number | null>(null);
  async function addMisses() {
    addingMisses = true;
    try {
      const res = await api.post(`/api/mock-test/${testId}/add-misses-to-srs`);
      missesAdded = res.added;
    } finally {
      addingMisses = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && phase === 'active') submit();
  }

  onDestroy(stopTimer);

  let secondsLeft = $derived(Math.ceil(remainingMs / 1000));
  let barPct = $derived((remainingMs / CLUE_MS) * 100);
</script>

<svelte:head><title>Mock Test — Jeopardy! Training</title></svelte:head>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-3xl mx-auto">
    {#if phase === 'idle'}
      <div class="bg-white rounded-xl shadow p-8 text-center">
        <h1 class="text-3xl font-bold text-jeopardy-blue mb-3">Anytime Test Simulator</h1>
        <p class="text-gray-600 mb-2">50 clues you've never seen · 15 seconds each · typed answers.</p>
        <p class="text-gray-500 text-sm mb-6">
          No feedback until the end — just like the real thing. Spelling is graded phonetically.
          The commonly-cited pass line is 35/50.
        </p>
        {#if error}<p class="text-red-600 mb-4">{error}</p>{/if}
        <button
          onclick={start}
          disabled={loading}
          class="px-8 py-3 bg-jeopardy-blue text-white font-bold rounded-lg hover:bg-blue-800 transition-colors disabled:opacity-50"
        >
          {hasResumable ? 'Resume Test' : 'Start Test'}
        </button>
      </div>
    {:else if phase === 'active' && clue}
      <div class="bg-white rounded-xl shadow p-8">
        <div class="flex items-center justify-between mb-4 text-sm text-gray-500">
          <span>Clue {position + 1} / {total}</span>
          <span class="font-mono text-lg {secondsLeft <= 5 ? 'text-red-600 font-bold' : 'text-gray-700'}">{secondsLeft}s</span>
        </div>
        <div class="h-1.5 bg-gray-100 rounded-full overflow-hidden mb-6">
          <div class="h-full bg-jeopardy-gold transition-none" style="width: {barPct}%"></div>
        </div>
        <p class="text-xs uppercase tracking-wide text-jeopardy-blue font-bold mb-2">{clue.category}</p>
        <p class="text-xl text-gray-900 mb-6">{clue.text}</p>
        <input
          bind:this={inputEl}
          bind:value={typed}
          onkeydown={onKeydown}
          disabled={submitting}
          placeholder="Type your answer…"
          autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck="false"
          class="w-full px-4 py-3 border-2 border-jeopardy-blue rounded-lg text-lg focus:outline-none focus:ring-2 focus:ring-jeopardy-gold"
        />
        <p class="text-xs text-gray-400 mt-2">Enter submits · auto-submits at 0:00 · don't phrase as a question</p>
      </div>
    {:else if phase === 'results' && results}
      <div class="bg-white rounded-xl shadow p-8 mb-6 text-center">
        <h1 class="text-2xl font-bold text-gray-800 mb-1">Score</h1>
        <p class="text-5xl font-extrabold {results.score >= results.passLine ? 'text-green-600' : 'text-jeopardy-blue'}">
          {results.score}/50
        </p>
        <p class="text-gray-500 mt-2">
          {results.score >= results.passLine
            ? `At or above the commonly-cited pass line (${results.passLine}).`
            : `${results.passLine - results.score} short of the commonly-cited pass line (${results.passLine}).`}
        </p>
        <div class="mt-4 flex justify-center gap-3">
          <button
            onclick={addMisses}
            disabled={addingMisses || missesAdded !== null}
            class="px-4 py-2 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 disabled:opacity-50"
          >
            {missesAdded !== null ? `${missesAdded} misses added to deck` : 'Add misses to SRS deck'}
          </button>
          <a href="/dashboard" class="px-4 py-2 rounded-lg border border-gray-300 text-sm font-semibold text-gray-700 hover:bg-gray-50">Dashboard</a>
        </div>
      </div>
      <div class="bg-white rounded-xl shadow divide-y divide-gray-100">
        {#each results.answers as row}
          <div class="p-4 flex gap-4 items-start">
            <span class="mt-1 shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold
              {row.finalCorrect ? 'bg-green-100 text-green-700' : 'bg-red-100 text-red-700'}">
              {row.finalCorrect ? '✓' : '✗'}
            </span>
            <div class="flex-1 min-w-0">
              <p class="text-xs uppercase text-gray-400">{row.category}</p>
              <p class="text-sm text-gray-800">{row.clue}</p>
              <p class="text-sm mt-1">
                <span class="text-gray-500">You:</span>
                <span class="{row.finalCorrect ? 'text-green-700' : 'text-red-700'} font-medium">{row.typed || '(no answer)'}</span>
                <span class="text-gray-400 mx-1">·</span>
                <span class="text-gray-500">Accepted:</span> <span class="font-medium">{row.accepted}</span>
                {#if row.overridden}<span class="ml-1 text-[10px] uppercase text-amber-600 font-bold">overridden</span>{/if}
              </p>
            </div>
            <button
              onclick={() => toggleOverride(row)}
              disabled={overridingPos !== null}
              class="shrink-0 text-xs px-2 py-1 rounded border border-gray-300 text-gray-600 hover:bg-gray-50 disabled:opacity-50"
            >
              Mark {row.finalCorrect ? 'wrong' : 'right'}
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
