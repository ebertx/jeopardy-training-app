<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import StatsChart from '$lib/components/StatsChart.svelte';
  import { onMount } from 'svelte';

  interface Stats {
    overall: { total: number; correct: number; accuracy: number };
    categoryBreakdown: Array<{ category: string; total: number; correct: number; accuracy: number }>;
    recentSessions: Array<{ id: number; started_at: string; completed_at: string; total: number; correct: number }>;
    dailyStats: Array<{ date: string; avgPercentage: number; sessionCount: number }>;
  }

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let stats = $state<Stats | null>(null);
  let loading = $state(true);
  let error = $state('');
  let includeReviewed = $state(false);

  async function fetchStats() {
    loading = true;
    error = '';
    try {
      stats = await api.get(`/api/stats?includeReviewed=${includeReviewed}`);
    } catch (err: any) {
      error = err?.message ?? 'Failed to load stats';
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    fetchStats();
  });

  // Re-fetch when toggle changes (but not on initial mount)
  let initialized = false;
  $effect(() => {
    // Access includeReviewed to track the dependency
    const _toggle = includeReviewed;
    if (!initialized) {
      initialized = true;
      return;
    }
    fetchStats();
  });

  // Chart data derived from stats
  let lineChartData = $derived(
    stats
      ? {
          labels: stats.dailyStats.map((d) => d.date),
          datasets: [
            {
              label: 'Accuracy %',
              data: stats.dailyStats.map((d) => d.avgPercentage),
              borderColor: '#0c47b7',
              borderWidth: 2,
              fill: false,
              tension: 0.3,
            },
          ],
        }
      : null
  );

  let lineChartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    scales: {
      y: {
        min: 0,
        max: 100,
        title: { display: true, text: 'Accuracy %' },
      },
      x: { ticks: { maxRotation: 45 } },
    },
  };

  let barChartData = $derived(
    stats
      ? {
          labels: stats.categoryBreakdown.map((c) => c.category),
          datasets: [
            {
              label: 'Accuracy %',
              data: stats.categoryBreakdown.map((c) => c.accuracy),
              backgroundColor: stats.categoryBreakdown.map((c) =>
                c.accuracy >= 75 ? '#22c55e' : c.accuracy >= 50 ? '#f59e0b' : '#ef4444'
              ),
              borderWidth: 1,
            },
          ],
        }
      : null
  );

  let barChartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    scales: {
      y: {
        min: 0,
        max: 100,
        title: { display: true, text: 'Accuracy %' },
      },
      x: { ticks: { maxRotation: 45 } },
    },
  };

  // Category breakdown sorted by accuracy ASC
  let sortedCategories = $derived(
    stats
      ? [...stats.categoryBreakdown].sort((a, b) => a.accuracy - b.accuracy)
      : []
  );
</script>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-6xl mx-auto">
    <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4 mb-8">
      <h1 class="text-3xl font-bold text-jeopardy-blue">Dashboard</h1>

      <!-- Toggle -->
      <label class="flex items-center gap-2 text-sm font-medium text-gray-700 cursor-pointer">
        <input
          type="checkbox"
          bind:checked={includeReviewed}
          class="w-4 h-4 rounded border-gray-300 text-jeopardy-blue focus:ring-jeopardy-blue"
        />
        Include Review Sessions
      </label>
    </div>

    <!-- Action Buttons -->
    <div class="flex flex-wrap gap-3 mb-8">
      <a
        href="/quiz"
        class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors"
      >
        Quiz
      </a>
      <a
        href="/review"
        class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors"
      >
        Review
      </a>
      <a
        href="/mastered"
        class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors"
      >
        Mastered
      </a>
      <a
        href="/coryat"
        class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors"
      >
        Coryat
      </a>
    </div>

    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg">
        {error}
      </div>
    {:else if stats}
      <!-- Overall Stats Cards -->
      <div class="flex flex-wrap gap-4 mb-8">
        <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
          <p class="text-sm font-medium text-gray-500 mb-1">Total Questions</p>
          <p class="text-3xl font-bold text-jeopardy-blue">{stats.overall.total.toLocaleString()}</p>
        </div>
        <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
          <p class="text-sm font-medium text-gray-500 mb-1">Correct Answers</p>
          <p class="text-3xl font-bold text-green-600">{stats.overall.correct.toLocaleString()}</p>
        </div>
        <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
          <p class="text-sm font-medium text-gray-500 mb-1">Overall Accuracy</p>
          <p class="text-3xl font-bold {stats.overall.accuracy >= 75 ? 'text-green-600' : stats.overall.accuracy >= 50 ? 'text-amber-500' : 'text-red-500'}">
            {stats.overall.accuracy.toFixed(1)}%
          </p>
        </div>
      </div>

      <!-- Daily Performance Chart -->
      {#if stats.dailyStats.length > 0 && lineChartData}
        <div class="bg-white rounded-xl shadow p-6 mb-8">
          <h2 class="text-lg font-semibold text-gray-800 mb-4">Daily Performance</h2>
          <div style="height: 300px;">
            <StatsChart type="line" data={lineChartData} options={lineChartOptions} />
          </div>
        </div>
      {:else}
        <div class="bg-white rounded-xl shadow p-6 mb-8 text-center text-gray-400">
          No daily performance data yet.
        </div>
      {/if}

      <!-- Category Performance Chart -->
      {#if stats.categoryBreakdown.length > 0 && barChartData}
        <div class="bg-white rounded-xl shadow p-6 mb-8">
          <h2 class="text-lg font-semibold text-gray-800 mb-4">Category Performance</h2>
          <div style="height: 300px;">
            <StatsChart type="bar" data={barChartData} options={barChartOptions} />
          </div>
        </div>
      {:else}
        <div class="bg-white rounded-xl shadow p-6 mb-8 text-center text-gray-400">
          No category data yet.
        </div>
      {/if}

      <!-- Category Detail Table -->
      {#if sortedCategories.length > 0}
        <div class="bg-white rounded-xl shadow p-6">
          <h2 class="text-lg font-semibold text-gray-800 mb-4">Category Breakdown</h2>
          <div class="overflow-x-auto">
            <table class="min-w-full text-sm">
              <thead>
                <tr class="border-b border-gray-200">
                  <th class="text-left py-3 px-4 font-semibold text-gray-600">Category</th>
                  <th class="text-right py-3 px-4 font-semibold text-gray-600">Total</th>
                  <th class="text-right py-3 px-4 font-semibold text-gray-600">Correct</th>
                  <th class="text-right py-3 px-4 font-semibold text-gray-600">Accuracy</th>
                </tr>
              </thead>
              <tbody>
                {#each sortedCategories as cat}
                  <tr class="border-b border-gray-100 hover:bg-gray-50 transition-colors">
                    <td class="py-3 px-4 text-gray-800">{cat.category}</td>
                    <td class="py-3 px-4 text-right text-gray-600">{cat.total}</td>
                    <td class="py-3 px-4 text-right text-gray-600">{cat.correct}</td>
                    <td class="py-3 px-4 text-right font-medium {cat.accuracy >= 75 ? 'text-green-600' : cat.accuracy >= 50 ? 'text-amber-500' : 'text-red-500'}">
                      {cat.accuracy.toFixed(1)}%
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>
      {/if}
    {/if}
  </div>
</div>
