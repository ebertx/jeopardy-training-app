<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import QuestionCard from '$lib/components/QuestionCard.svelte';
  import CategoryFilter from '$lib/components/CategoryFilter.svelte';

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let queryInput = $state('');
  let activeQuery = $state('');
  let started = $state(false);

  let question = $state<any>(null);
  let isNew = $state(false);
  let matchCount = $state(0);
  let remaining = $state(0);
  let done = $state(false);
  let showAnswer = $state(false);
  let loading = $state(false);
  let error = $state('');
  let sessionId = $state<number | null>(null);
  let submitting = $state(false);
  let runningStats = $state({ total: 0, correct: 0 });

  let categories = $state<Array<{ name: string; count: number }>>([]);
  let selectedCategory = $state('all');
  let gameTypeFilters = $state<string[]>([]);
  let filtersOpen = $state(false);
  let filterGen = $state(0);

  let accuracy = $derived(
    runningStats.total > 0 ? Math.round((runningStats.correct / runningStats.total) * 100) : 0
  );

  function buildParams(): URLSearchParams {
    const params = new URLSearchParams();
    params.set('q', activeQuery);
    if (selectedCategory !== 'all') params.set('category', selectedCategory);
    if (gameTypeFilters.length > 0) params.set('gameTypes', gameTypeFilters.join(','));
    return params;
  }

  async function fetchNext() {
    const gen = filterGen;
    loading = true;
    error = '';
    try {
      const res = await api.get(`/api/drill/next?${buildParams()}`);
      if (gen !== filterGen) return;
      matchCount = res.matchCount ?? 0;
      remaining = res.remaining ?? 0;
      if (res.done) {
        done = true;
        question = null;
      } else {
        done = false;
        isNew = res.isNew;
        question = res.card;
      }
    } catch (err: any) {
      if (gen !== filterGen) return;
      error = err?.message ?? 'Failed to load clue';
    } finally {
      if (gen === filterGen) loading = false;
    }
  }

  async function startDrill() {
    const q = queryInput.trim();
    if (!q) return;
    activeQuery = q;
    started = true;
    filterGen++;
    done = false;
    showAnswer = false;
    runningStats = { total: 0, correct: 0 };
    await fetchNext();
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
      showAnswer = false;
      await fetchNext();
    } catch (err: any) {
      error = err?.message ?? 'Failed to submit answer';
    } finally {
      submitting = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
    if (!started) return;
    if (e.code === 'Space' && !showAnswer) {
      e.preventDefault();
      showAnswer = true;
    } else if (showAnswer && !submitting) {
      if (e.code === 'Digit1') handleGrade('wrong');
      else if (e.code === 'Digit2') handleGrade('got_it');
      else if (e.code === 'Digit3') handleGrade('too_easy');
    }
  }

  onMount(async () => {
    try {
      const [cats, prefs] = await Promise.all([
        api.get('/api/categories'),
        api.get('/api/preferences'),
      ]);
      categories = cats ?? [];
      gameTypeFilters = prefs?.gameTypeFilters ?? [];
    } catch {
      // Non-critical
    }
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="min-h-screen bg-gray-50 py-3 sm:py-6 px-4">
  <div class="max-w-2xl mx-auto flex flex-col gap-3 sm:gap-4">
    <div class="flex items-center gap-2 flex-wrap">
      <h1 class="text-xl sm:text-2xl font-bold text-jeopardy-blue">Drill</h1>
      {#if started && !done}
        <div class="text-sm font-medium text-gray-600">
          <span class="font-bold text-jeopardy-blue">{matchCount}</span> match ·
          <span class="font-bold text-jeopardy-blue">{remaining}</span> to hit now
        </div>
      {/if}
      <div class="flex items-center gap-2 ml-auto">
        {#if runningStats.total > 0}
          <div class="text-sm text-gray-500">
            {runningStats.correct}/{runningStats.total} ({accuracy}%)
          </div>
        {/if}
        <button
          onclick={() => goto('/dashboard')}
          class="px-3 py-1.5 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100 transition-colors"
        >
          Done
        </button>
      </div>
    </div>

    <!-- Search + filters -->
    <form onsubmit={(e) => { e.preventDefault(); startDrill(); }} class="flex flex-col gap-3 bg-white rounded-xl shadow-sm px-4 py-3">
      <div class="flex gap-2">
        <input
          type="search"
          bind:value={queryInput}
          placeholder="Search a topic — e.g. Impressionism, Marie Curie"
          class="flex-1 min-w-0 rounded-lg border border-gray-300 px-3 py-2 text-sm focus:border-jeopardy-blue focus:outline-none focus:ring-1 focus:ring-jeopardy-blue"
        />
        <button type="submit" class="shrink-0 px-4 py-2 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 transition-colors">
          Drill
        </button>
        <button type="button" onclick={() => (filtersOpen = !filtersOpen)} class="shrink-0 px-3 py-2 rounded-lg border border-gray-300 text-sm text-gray-700 hover:bg-gray-100">
          Filters
        </button>
      </div>
      {#if filtersOpen}
        <div class="flex flex-col sm:flex-row sm:items-center gap-4 border-t border-gray-100 pt-3">
          <div class="flex-1">
            <p class="block text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Category</p>
            <CategoryFilter {categories} selected={selectedCategory} onchange={(v) => (selectedCategory = v)} />
          </div>
          <div>
            <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2">Exclude Game Types</p>
            <div class="flex flex-wrap gap-3">
              {#each ['Kids', 'Teen', 'College'] as type}
                <label class="flex items-center gap-1.5 text-sm text-gray-700 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={gameTypeFilters.includes(type)}
                    onchange={() => {
                      gameTypeFilters = gameTypeFilters.includes(type)
                        ? gameTypeFilters.filter((t) => t !== type)
                        : [...gameTypeFilters, type];
                    }}
                    class="w-4 h-4 rounded border-gray-300 text-jeopardy-blue focus:ring-jeopardy-blue"
                  />
                  {type}
                </label>
              {/each}
            </div>
          </div>
        </div>
      {/if}
    </form>

    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline text-red-500">Dismiss</button>
      </div>
    {/if}

    {#if loading && !question}
      <div class="flex justify-center py-20">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if question}
      <div class="min-h-[420px]">
        <QuestionCard
          clue={question.answer}
          answer={question.question}
          category={question.category}
          classifierCategory={question.classifier_category ?? question.category}
          clueValue={question.clue_value ?? null}
          round={question.round ?? null}
          airDate={question.air_date ?? null}
          {showAnswer}
          onRevealAnswer={() => { showAnswer = true; }}
          onWrong={() => handleGrade('wrong')}
          onGotIt={() => handleGrade('got_it')}
          onTooEasy={() => handleGrade('too_easy')}
          {submitting}
        />
      </div>
      <p class="hidden sm:block text-center text-xs text-gray-400">
        {#if !showAnswer}
          Press <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">Space</kbd> to reveal
        {:else}
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">1</kbd> Wrong ·
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">2</kbd> Got it ·
          <kbd class="px-1.5 py-0.5 bg-gray-100 rounded border border-gray-300 font-mono">3</kbd> Too easy
        {/if}
      </p>
    {:else if started && done}
      <div class="text-center py-16 text-gray-600">
        🎯 You've drilled everything due or new for “{activeQuery}”.
      </div>
    {:else}
      <div class="text-center py-16 text-gray-500">
        Search a topic above to start drilling.
      </div>
    {/if}
  </div>
</div>
