<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  interface Topic {
    topic: string;
    explanation: string;
    readings: string[];
    wikipedia: string[];
    strategies: string[];
  }

  interface Recommendation {
    id: number;
    generated_at: string;
    days_analyzed: number;
    analysis: string;
    recommendations: { topics: Topic[] };
    question_count: number;
  }

  let daysInput = $state(30);
  let generating = $state(false);
  let generateError = $state('');

  let latest = $state<Recommendation | null>(null);
  let latestLoading = $state(true);
  let latestError = $state('');

  let history = $state<Recommendation[]>([]);
  let historyLoading = $state(true);
  let historyError = $state('');
  let expandedHistoryIds = $state<Set<number>>(new Set());

  function toggleHistory(id: number) {
    const next = new Set(expandedHistoryIds);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    expandedHistoryIds = next;
  }

  function formatDate(dateStr: string): string {
    return new Date(dateStr).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  async function fetchLatest() {
    latestLoading = true;
    latestError = '';
    try {
      latest = await api.get('/api/study/latest');
    } catch (err: any) {
      if (err?.status === 404) {
        latest = null;
      } else {
        latestError = err?.message ?? 'Failed to load latest recommendation';
      }
    } finally {
      latestLoading = false;
    }
  }

  async function fetchHistory() {
    historyLoading = true;
    historyError = '';
    try {
      history = await api.get('/api/study/history');
    } catch (err: any) {
      historyError = err?.message ?? 'Failed to load history';
    } finally {
      historyLoading = false;
    }
  }

  async function handleGenerate() {
    if (generating) return;
    generating = true;
    generateError = '';
    try {
      const result = await api.post('/api/study/generate', { days: daysInput });
      latest = result;
      // Refresh history too
      fetchHistory();
    } catch (err: any) {
      generateError = err?.message ?? 'Failed to generate recommendations';
    } finally {
      generating = false;
    }
  }

  onMount(() => {
    fetchLatest();
    fetchHistory();
  });
</script>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-3xl mx-auto flex flex-col gap-8">

    <h1 class="text-3xl font-bold text-jeopardy-blue">Study Recommendations</h1>

    <!-- Generate Form -->
    <div class="bg-white rounded-xl shadow p-6 flex flex-col gap-4">
      <h2 class="text-lg font-semibold text-gray-800">Generate New Recommendations</h2>
      <p class="text-sm text-gray-600">Analyze your recent quiz history and get AI-powered study suggestions.</p>

      <div class="flex flex-col sm:flex-row sm:items-end gap-3">
        <div class="flex flex-col gap-1">
          <label for="days-input" class="text-sm font-medium text-gray-700">Days to analyze</label>
          <input
            id="days-input"
            type="number"
            min="1"
            max="365"
            bind:value={daysInput}
            class="w-32 px-3 py-2 border border-gray-300 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-jeopardy-blue"
          />
        </div>
        <button
          onclick={handleGenerate}
          disabled={generating}
          class="px-5 py-2 bg-jeopardy-blue hover:bg-blue-800 disabled:opacity-60 text-white font-semibold rounded-lg transition-colors flex items-center gap-2"
        >
          {#if generating}
            <span class="animate-spin rounded-full h-4 w-4 border-b-2 border-white inline-block"></span>
            Analyzing with AI...
          {:else}
            Generate Recommendations
          {/if}
        </button>
      </div>

      {#if generateError}
        <div class="px-3 py-2 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
          {generateError}
        </div>
      {/if}
    </div>

    <!-- Latest Recommendation -->
    <div class="flex flex-col gap-4">
      <h2 class="text-xl font-semibold text-gray-800">Latest Recommendation</h2>

      {#if latestLoading}
        <div class="flex justify-center py-8">
          <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-jeopardy-blue"></div>
        </div>
      {:else if latestError}
        <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
          {latestError}
        </div>
      {:else if latest === null}
        <div class="bg-white rounded-xl shadow p-6 text-center text-gray-500">
          No recommendations yet. Generate your first one above!
        </div>
      {:else}
        <div class="text-xs text-gray-500 -mb-2">
          Generated {formatDate(latest.generated_at)} &bull; {latest.days_analyzed} days analyzed &bull; {latest.question_count} questions
        </div>

        <!-- Analysis summary -->
        <div class="bg-indigo-50 border border-indigo-200 rounded-xl p-4">
          <p class="text-indigo-800 text-sm leading-relaxed whitespace-pre-line">{latest.analysis}</p>
        </div>

        <!-- Topic cards -->
        {#each latest.recommendations.topics as topic}
          <div class="bg-white rounded-xl shadow p-5 flex flex-col gap-3">
            <h3 class="text-base font-bold text-gray-800">{topic.topic}</h3>
            <p class="text-sm text-gray-600 leading-relaxed">{topic.explanation}</p>

            {#if topic.readings && topic.readings.length > 0}
              <div>
                <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Readings</p>
                <ul class="flex flex-col gap-1">
                  {#each topic.readings as reading}
                    <li class="text-sm text-gray-700">📚 {reading}</li>
                  {/each}
                </ul>
              </div>
            {/if}

            {#if topic.wikipedia && topic.wikipedia.length > 0}
              <div>
                <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Wikipedia</p>
                <ul class="flex flex-col gap-1">
                  {#each topic.wikipedia as wikiUrl}
                    <li>
                      <a
                        href={wikiUrl}
                        target="_blank"
                        rel="noopener noreferrer"
                        class="text-sm text-blue-600 hover:underline break-all"
                      >
                        {wikiUrl}
                      </a>
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}

            {#if topic.strategies && topic.strategies.length > 0}
              <div>
                <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Strategies</p>
                <ul class="flex flex-col gap-1">
                  {#each topic.strategies as strategy}
                    <li class="text-sm text-gray-700">💡 {strategy}</li>
                  {/each}
                </ul>
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>

    <!-- History Section -->
    <div class="flex flex-col gap-3">
      <h2 class="text-xl font-semibold text-gray-800">History</h2>

      {#if historyLoading}
        <div class="flex justify-center py-6">
          <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-jeopardy-blue"></div>
        </div>
      {:else if historyError}
        <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
          {historyError}
        </div>
      {:else if history.length === 0}
        <div class="bg-white rounded-xl shadow p-6 text-center text-gray-500">
          No recommendation history yet.
        </div>
      {:else}
        {#each history as rec}
          <div class="bg-white rounded-xl shadow overflow-hidden">
            <button
              class="w-full flex items-center justify-between px-5 py-4 hover:bg-gray-50 transition-colors text-left"
              onclick={() => toggleHistory(rec.id)}
            >
              <div>
                <p class="font-semibold text-gray-800 text-sm">{formatDate(rec.generated_at)}</p>
                <p class="text-xs text-gray-500 mt-0.5">
                  {rec.days_analyzed} days &bull; {rec.question_count} questions &bull; {rec.recommendations.topics.length} topics
                </p>
              </div>
              <span class="text-gray-400 text-lg">{expandedHistoryIds.has(rec.id) ? '▲' : '▼'}</span>
            </button>

            {#if expandedHistoryIds.has(rec.id)}
              <div class="border-t border-gray-100 px-5 py-4 flex flex-col gap-3">
                <p class="text-sm text-gray-600 leading-relaxed italic">{rec.analysis}</p>
                <ul class="flex flex-col gap-1">
                  {#each rec.recommendations.topics as topic}
                    <li class="text-sm text-gray-700 font-medium">&bull; {topic.topic}</li>
                  {/each}
                </ul>
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>

  </div>
</div>
