<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import QuestionCard from '$lib/components/QuestionCard.svelte';
  import CategoryFilter from '$lib/components/CategoryFilter.svelte';
  import SessionSummary from '$lib/components/SessionSummary.svelte';
  import Modal from '$lib/components/Modal.svelte';
  import CountdownTimer from '$lib/components/CountdownTimer.svelte';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  // --- State ---
  let question = $state<any>(null);
  let isNew = $state(false);
  let nextDueAt = $state<string | null>(null);
  let dueSoonCount = $state(0);
  let dueCount = $state(0);
  let newRemaining = $state(0);
  let done = $state(false);
  let showAnswer = $state(false);
  let loading = $state(true);
  let error = $state('');
  let sessionId = $state<number | null>(null);
  let categories = $state<Array<{ name: string; count: number }>>([]);
  let selectedCategory = $state('all');
  let runningStats = $state({ total: 0, correct: 0 });
  let gameTypeFilters = $state<string[]>([]);
  let showSessionSummary = $state(false);
  let sessionSummary = $state<any>(null);
  let submitting = $state(false);
  let showEndConfirm = $state(false);
  let filtersOpen = $state(false);
  let pausedForInsight = $state(false);
  let insight = $state<{ insight: string; hook: string } | null>(null);
  let insightLoading = $state(false);
  let insightShown = $state(false); // Explain-on-correct inline display

  // Incremented on every filter change; in-flight fetches/prefetches captured
  // before the change discard their results to avoid leaking old-filter data.
  let filterGen = $state(0);

  // Derived accuracy
  let accuracy = $derived(
    runningStats.total > 0
      ? Math.round((runningStats.correct / runningStats.total) * 100)
      : 0
  );

  let activeFilterCount = $derived(
    (selectedCategory !== 'all' ? 1 : 0) + gameTypeFilters.length
  );

  // --- API helpers ---
  function buildQuizParams(): URLSearchParams {
    const params = new URLSearchParams();
    if (selectedCategory !== 'all') params.set('category', selectedCategory);
    if (gameTypeFilters.length > 0) params.set('gameTypes', gameTypeFilters.join(','));
    return params;
  }

  async function fetchQuestion() {
    const gen = filterGen;
    loading = true;
    error = '';
    try {
      const res = await api.get(`/api/practice/next?${buildQuizParams()}`);
      if (gen !== filterGen) return;
      dueCount = res.dueCount ?? 0;
      newRemaining = res.newRemaining ?? 0;
      if (res.done) {
        done = true;
        question = null;
        nextDueAt = res.nextDueAt ?? null;
        dueSoonCount = res.dueSoonCount ?? 0;
        pausedForInsight = false;
        insightShown = false;
        insight = null;
      } else {
        done = false;
        isNew = res.isNew;
        question = res.card;
        pausedForInsight = false;
        insightShown = false;
        insight = null;
      }
    } catch (err: any) {
      if (gen !== filterGen) return;
      error = err?.message ?? 'Failed to load question';
    } finally {
      if (gen === filterGen) loading = false;
    }
  }

  async function handleGrade(rating: 'wrong' | 'got_it' | 'too_easy') {
    if (submitting || !question) return;
    submitting = true;
    try {
      const result = await api.post('/api/practice/grade', {
        questionId: question.id,
        rating,
        sessionId,
      });
      sessionId = result.sessionId;
      runningStats.total++;
      if (rating !== 'wrong') runningStats.correct++;
      if (rating === 'wrong') {
        // Teaching pause: stay on the card and show the insight.
        pausedForInsight = true;
        fetchInsight(question.id);
      } else {
        showAnswer = false;
        await fetchQuestion();
      }
    } catch (err: any) {
      error = err?.message ?? 'Failed to submit answer';
    } finally {
      submitting = false;
    }
  }

  async function fetchInsight(questionId: number) {
    insight = null;
    insightLoading = true;
    try {
      insight = await api.get(`/api/insight/${questionId}`);
    } catch {
      insight = null; // 404 (disabled) or failure: pause just shows Next
    } finally {
      insightLoading = false;
    }
  }

  async function advanceFromPause() {
    pausedForInsight = false;
    insight = null;
    insightShown = false;
    showAnswer = false;
    await fetchQuestion();
  }

  async function handleExplain() {
    if (!question || insightShown) return;
    insightShown = true;
    await fetchInsight(question.id);
  }

  async function handleArchive() {
    if (!question) return;
    try {
      await api.post(`/api/questions/${question.id}/archive`, {
        reason: 'Missing media or problematic question',
      });
      // Move to next question
      showAnswer = false;
      await fetchQuestion();
    } catch (err: any) {
      error = err?.message ?? 'Failed to archive question';
    }
  }

  async function handleEndSession() {
    if (!sessionId) {
      showEndConfirm = false;
      goto('/dashboard');
      return;
    }
    try {
      const result = await api.post('/api/quiz/complete', { sessionId });
      sessionSummary = result.summary;
      showSessionSummary = true;
    } catch (err: any) {
      error = err?.message ?? 'Failed to end session';
    } finally {
      showEndConfirm = false;
    }
  }

  async function savePreferences() {
    try {
      await api.put('/api/preferences', { gameTypeFilters });
    } catch {
      // Non-critical
    }
  }

  async function handleCategoryChange(value: string) {
    selectedCategory = value;
    filterGen++;
    showAnswer = false;
    await fetchQuestion();
  }

  function toggleGameTypeFilter(type: string) {
    if (gameTypeFilters.includes(type)) {
      gameTypeFilters = gameTypeFilters.filter((t) => t !== type);
    } else {
      gameTypeFilters = [...gameTypeFilters, type];
    }
    savePreferences();
    filterGen++;
    showAnswer = false;
    fetchQuestion();
  }

  // --- Keyboard shortcuts ---
  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
    // Modals own Escape and focus trap themselves; don't process answer keys while open.
    if (showEndConfirm || showSessionSummary) return;

    if (pausedForInsight) {
      if (e.code === 'Space') e.preventDefault();
      if (['Space', 'Enter', 'Digit1', 'Digit2', 'Digit3'].includes(e.code)) {
        advanceFromPause();
      }
      return;
    }

    if (e.code === 'Space' && !showAnswer) {
      e.preventDefault();
      showAnswer = true;
    } else if (showAnswer && !submitting) {
      if (e.code === 'Digit1') handleGrade('wrong');
      else if (e.code === 'Digit2') handleGrade('got_it');
      else if (e.code === 'Digit3') handleGrade('too_easy');
    }
  }

  // --- Mount ---
  onMount(async () => {
    try {
      const [cats, prefs] = await Promise.all([
        api.get('/api/categories'),
        api.get('/api/preferences'),
      ]);
      categories = cats ?? [];
      gameTypeFilters = prefs?.gameTypeFilters ?? [];
    } catch {
      // Non-critical; continue
    }
    await fetchQuestion();
  });
</script>
<svelte:head>
  <title>Practice — Jeopardy! Training</title>
</svelte:head>


<svelte:window onkeydown={handleKeydown} />

<div class="min-h-screen bg-gray-50 py-3 sm:py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-3 sm:gap-4">

    <!-- Header row (single line on mobile) -->
    <div class="flex items-center gap-2 flex-wrap">
      <h1 class="text-xl sm:text-2xl font-bold text-jeopardy-blue">Practice</h1>

      <div class="text-sm font-medium text-gray-600">
        Due <span class="font-bold text-jeopardy-blue">{dueCount}</span>
        · New left <span class="font-bold text-jeopardy-blue">{newRemaining}</span>
      </div>

      {#if runningStats.total > 0}
        <div class="text-sm font-medium text-gray-600">
          <span class="text-green-600 font-bold">{runningStats.correct}</span>/{runningStats.total}
          (<span class="{accuracy >= 75 ? 'text-green-600' : accuracy >= 50 ? 'text-amber-500' : 'text-red-500'} font-bold">{accuracy}%</span>)
        </div>
      {/if}

      <div class="flex items-center gap-2 ml-auto">
        <button
          onclick={() => (filtersOpen = !filtersOpen)}
          aria-expanded={filtersOpen}
          class="px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
        >
          Filters{activeFilterCount > 0 ? ` (${activeFilterCount})` : ''}
        </button>

        <button
          onclick={() => (sessionId ? (showEndConfirm = true) : goto('/dashboard'))}
          class="px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
        >
          End
        </button>
      </div>
    </div>

    <!-- Filters (collapsible) -->
    {#if filtersOpen}
      <div class="bg-white rounded-xl shadow-sm px-5 py-4 flex flex-col sm:flex-row sm:items-center gap-4">
        <div class="flex-1">
          <p class="block text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Category</p>
          <CategoryFilter
            {categories}
            selected={selectedCategory}
            onchange={handleCategoryChange}
          />
        </div>

        <div>
          <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2">Exclude Game Types</p>
          <div class="flex flex-wrap gap-3">
            {#each ['Kids', 'Teen', 'College'] as type}
              <label class="flex items-center gap-1.5 text-sm text-gray-700 cursor-pointer">
                <input
                  type="checkbox"
                  checked={gameTypeFilters.includes(type)}
                  onchange={() => toggleGameTypeFilter(type)}
                  class="w-4 h-4 rounded border-gray-300 text-jeopardy-blue focus:ring-jeopardy-blue"
                />
                {type}
              </label>
            {/each}
          </div>
        </div>
      </div>
    {/if}

    <!-- Error banner -->
    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline text-red-500">Dismiss</button>
      </div>
    {/if}

    <!-- Question card -->
    {#if loading && !question}
      <div class="flex justify-center py-20">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if question}
      <div class="min-h-[420px]">
        <div class="flex justify-end mb-1">
          <CountdownTimer resetKey={question.id} running={!showAnswer} />
        </div>
        <QuestionCard
          clue={question.answer}
          answer={question.question}
          category={question.category}
          classifierCategory={question.classifier_category ?? question.classifierCategory ?? question.category}
          clueValue={question.clue_value ?? question.clueValue ?? null}
          round={question.round ?? null}
          airDate={question.air_date ?? question.airDate ?? null}
          {showAnswer}
          onRevealAnswer={() => { showAnswer = true; }}
          onWrong={() => handleGrade('wrong')}
          onGotIt={() => handleGrade('got_it')}
          onTooEasy={() => handleGrade('too_easy')}
          {submitting}
          paused={pausedForInsight}
        >
          {#snippet badge()}
            {#if isNew}
              <span class="inline-block px-2 py-0.5 rounded-full bg-jeopardy-gold text-jeopardy-blue text-xs font-bold uppercase tracking-wide">New</span>
            {/if}
          {/snippet}
          {#snippet pausePanel()}
            <div class="flex flex-col gap-3">
              {#if insightLoading}
                <div class="flex items-center gap-2 text-white/70 text-sm py-2">
                  <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-jeopardy-gold"></div>
                  Finding the lesson…
                </div>
              {:else if insight}
                <div class="bg-white/10 border border-white/20 rounded-xl px-4 py-3 text-left">
                  <p class="text-white/90 text-sm leading-relaxed">{insight.insight}</p>
                  <p class="text-jeopardy-gold text-sm font-semibold mt-2">💡 {insight.hook}</p>
                </div>
              {/if}
              <button
                onclick={advanceFromPause}
                class="w-full py-3 rounded-xl bg-white/10 hover:bg-white/20 border border-white/20 text-white font-semibold text-lg transition-colors"
              >
                Next →
              </button>
            </div>
          {/snippet}
          {#snippet additionalActions()}
            {#if showAnswer && !pausedForInsight}
              {#if !insightShown}
                <button
                  onclick={handleExplain}
                  class="w-full py-2 mb-2 rounded-lg bg-white/10 hover:bg-white/20 border border-white/20 text-white/80 text-sm font-medium transition-colors"
                >
                  Explain this one
                </button>
              {:else if insightLoading}
                <p class="text-white/60 text-sm text-center py-2">Finding the lesson…</p>
              {:else if insight}
                <div class="bg-white/10 border border-white/20 rounded-xl px-4 py-3 text-left mb-2">
                  <p class="text-white/90 text-sm leading-relaxed">{insight.insight}</p>
                  <p class="text-jeopardy-gold text-sm font-semibold mt-2">💡 {insight.hook}</p>
                </div>
              {/if}
              <button
                onclick={handleArchive}
                class="w-full py-2 rounded-lg bg-white/10 hover:bg-white/20 border border-white/20 text-white/80 text-sm font-medium transition-colors"
              >
                Archive (problematic question)
              </button>
            {/if}
          {/snippet}
        </QuestionCard>
      </div>

      <!-- Keyboard hint (desktop only) -->
      <p class="hidden sm:block text-center text-xs text-gray-400">
        {#if !showAnswer}
          Press <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">Space</kbd> to reveal answer
        {:else if pausedForInsight}
          Press any grade key or Space for next clue
        {:else}
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">1</kbd> Wrong &nbsp;
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">2</kbd> Got it &nbsp;
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">3</kbd> Too easy
        {/if}
      </p>
    {:else if done}
      <div class="text-center py-16 text-gray-600 flex flex-col items-center gap-3">
        {#if dueSoonCount > 0 && nextDueAt}
          <p>
            ✅ Caught up for now — <span class="font-semibold">{dueSoonCount}</span> more
            {dueSoonCount === 1 ? 'card comes' : 'cards come'} due at
            <span class="font-semibold">{new Date(nextDueAt).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</span>.
          </p>
          <button
            onclick={() => fetchQuestion()}
            class="px-4 py-2 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
          >
            Check again
          </button>
        {:else}
          <p>🎉 All caught up — no reviews due and today's new-clue limit is reached.</p>
          {#if nextDueAt}
            <p class="text-sm text-gray-400">Next review due {new Date(nextDueAt).toLocaleString([], { weekday: 'short', hour: 'numeric', minute: '2-digit' })}.</p>
          {/if}
        {/if}
      </div>
    {:else}
      <div class="text-center py-16 text-gray-500">No questions available for the selected filters.</div>
    {/if}

  </div>
</div>

<!-- End session confirmation -->
{#if showEndConfirm}
  <Modal onclose={() => (showEndConfirm = false)} ariaLabelledby="end-session-title">
    <div class="rounded-2xl bg-white shadow-2xl p-6 flex flex-col gap-4">
      <h2 id="end-session-title" class="text-lg font-bold text-gray-800">End Session?</h2>
      <p class="text-sm text-gray-600">This will complete your current quiz session and show a summary.</p>
      <div class="flex gap-3">
        <button
          onclick={() => (showEndConfirm = false)}
          class="flex-1 py-2.5 rounded-xl border border-gray-300 text-gray-700 text-sm font-medium hover:bg-gray-50 transition-colors"
        >
          Cancel
        </button>
        <button
          onclick={handleEndSession}
          class="flex-1 py-2.5 rounded-xl bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 transition-colors"
        >
          End Session
        </button>
      </div>
    </div>
  </Modal>
{/if}

<!-- Session summary modal -->
{#if showSessionSummary && sessionSummary}
  <SessionSummary
    summary={sessionSummary}
    onclose={() => {
      showSessionSummary = false;
      sessionId = null;
      runningStats = { total: 0, correct: 0 };
      goto('/dashboard');
    }}
  />
{/if}
