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
    dailyAccuracy: Array<{ date: string; total: number; correct: number; accuracy: number }>;
  }

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let stats = $state<Stats | null>(null);
  let loading = $state(true);
  let error = $state('');
  let includeReviewed = $state(false);

  let srs = $state<{ dueCount: number; newRemaining: number; reviewedToday: number; forecast: Array<{ date: string; count: number }> } | null>(null);

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
    api.get('/api/practice/status').then((s) => (srs = s)).catch(() => (srs = null));
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

  // Daily accuracy (last 30 days), computed from every attempt — unlike the old
  // dailyStats series it doesn't depend on sessions being formally completed.
  let lineChartData = $derived(
    stats?.dailyAccuracy?.length
      ? {
          labels: stats.dailyAccuracy.map((d) => d.date),
          datasets: [
            {
              label: 'Accuracy %',
              data: stats.dailyAccuracy.map((d) => d.accuracy),
              borderColor: '#0c47b7',
              borderWidth: 2,
              pointRadius: 3,
              pointHitRadius: 12,
              pointBackgroundColor: '#0c47b7',
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
    plugins: { legend: { display: false } },
    scales: {
      y: {
        min: 0,
        max: 100,
        title: { display: true, text: 'Accuracy %' },
      },
      x: { ticks: { maxRotation: 45 } },
    },
  };

  // 14-day due forecast, padded so quiet days render as true zeros.
  let forecastChartData = $derived.by(() => {
    if (!srs || !srs.forecast) return null;
    const counts = new Map(srs.forecast.map((f) => [f.date, f.count]));
    const start = new Date();
    const labels: string[] = [];
    const data: number[] = [];
    for (let i = 0; i < 14; i++) {
      const d = new Date(Date.UTC(start.getUTCFullYear(), start.getUTCMonth(), start.getUTCDate() + i));
      const key = d.toISOString().slice(0, 10);
      labels.push(i === 0 ? 'Today' : d.toLocaleDateString([], { weekday: 'short', day: 'numeric', timeZone: 'UTC' }));
      data.push(counts.get(key) ?? 0);
    }
    return {
      labels,
      datasets: [
        {
          label: 'Reviews due',
          data,
          backgroundColor: '#0c47b7',
          borderRadius: 4,
          maxBarThickness: 28,
        },
      ],
    };
  });

  let forecastChartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: { legend: { display: false } },
    scales: {
      y: { min: 0, ticks: { precision: 0 } },
      x: { grid: { display: false } },
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
<svelte:head>
  <title>Dashboard — Jeopardy! Training</title>
</svelte:head>


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

    <!-- SRS Practice Summary -->
    {#if srs}
      <div class="bg-white rounded-xl shadow-sm p-5 mb-8">
        <div class="flex flex-wrap gap-8">
          <div>
            <p class="text-3xl font-bold text-jeopardy-blue">{srs.dueCount}</p>
            <p class="text-xs uppercase text-gray-500">Due today</p>
          </div>
          <div>
            <p class="text-3xl font-bold text-jeopardy-blue">{srs.newRemaining}</p>
            <p class="text-xs uppercase text-gray-500">New left</p>
          </div>
          <div>
            <p class="text-3xl font-bold text-jeopardy-blue">{srs.reviewedToday}</p>
            <p class="text-xs uppercase text-gray-500">Reviewed today</p>
          </div>
          <a
            href="/practice"
            class="ml-auto self-center px-4 py-2 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 transition-colors"
          >
            Practice &rarr;
          </a>
        </div>
        {#if forecastChartData}
          <div class="mt-5 pt-4 border-t border-gray-100">
            <h2 class="text-sm font-semibold text-gray-600 mb-2">Reviews due — next 14 days</h2>
            <div class="h-36">
              <StatsChart type="bar" data={forecastChartData} options={forecastChartOptions} />
            </div>
          </div>
        {/if}
      </div>
    {/if}

    <!-- Action Buttons -->
    <div class="flex flex-wrap gap-3 mb-8">
      <a
        href="/practice"
        class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors"
      >
        Practice
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

      <!-- Daily Accuracy Chart -->
      {#if lineChartData}
        <div class="bg-white rounded-xl shadow p-6 mb-8">
          <h2 class="text-lg font-semibold text-gray-800 mb-4">Accuracy — last 30 days</h2>
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
