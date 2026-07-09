<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import StatsChart from '$lib/components/StatsChart.svelte';
  import { onMount } from 'svelte';

  interface KindStat { total: number; correct: number; accuracy: number }
  interface Stats {
    overall: KindStat;
    cold: KindStat;
    review: KindStat;
    cold30d: KindStat;
    mockReadiness: { tests: Array<{ id: number; completedAt: string; score: number }>; best: number | null; latest: number | null; passLine: number };
    categoryBreakdown: Array<{ category: string; total: number; correct: number; accuracy: number;
      coldTotal: number; coldCorrect: number; coldAccuracy: number;
      reviewTotal: number; reviewCorrect: number; reviewAccuracy: number }>;
    dailyAccuracy: Array<{ date: string; total: number; correct: number; accuracy: number;
      coldTotal: number; coldCorrect: number; coldAccuracy: number;
      reviewTotal: number; reviewCorrect: number; reviewAccuracy: number }>;
  }

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let stats = $state<Stats | null>(null);
  let loading = $state(true);
  let error = $state('');

  let srs = $state<{
    dueCount: number;
    newRemaining: number;
    reviewedToday: number;
    forecast: Array<{ date: string; count: number }>;
    adaptiveWeights?: Array<{ category: string; attempts: number; accuracy: number; weight: number }>;
    deck?: { learning: number; mastered: number; struggling: number };
  } | null>(null);

  let blindspots = $state<{
    packs: Array<{ id: number; theme: string; diagnosis: string }>;
    insufficientData: boolean;
    configured: boolean;
  } | null>(null);

  async function fetchStats() {
    loading = true;
    error = '';
    try {
      stats = await api.get('/api/stats');
    } catch (err: any) {
      error = err?.message ?? 'Failed to load stats';
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    fetchStats();
    api.get('/api/practice/status').then((s) => (srs = s)).catch(() => (srs = null));
    api
      .get('/api/blindspots')
      .then((b) => (blindspots = b))
      .catch(() => (blindspots = null));
  });

  // Daily accuracy (last 30 days), computed from every attempt — unlike the old
  // dailyStats series it doesn't depend on sessions being formally completed.
  let lineChartData = $derived(
    stats?.dailyAccuracy?.length
      ? {
          labels: stats.dailyAccuracy.map((d) => d.date),
          datasets: [
            {
              label: 'Cold (first attempt) %',
              data: stats.dailyAccuracy.map((d) => (d.coldTotal > 0 ? d.coldAccuracy : null)),
              borderColor: '#0c47b7',
              borderWidth: 2.5,
              pointRadius: 3,
              pointBackgroundColor: '#0c47b7',
              fill: false,
              tension: 0.3,
              spanGaps: true,
            },
            {
              label: 'Review %',
              data: stats.dailyAccuracy.map((d) => (d.reviewTotal > 0 ? d.reviewAccuracy : null)),
              borderColor: '#9ca3af',
              borderWidth: 1.5,
              pointRadius: 2,
              pointBackgroundColor: '#9ca3af',
              fill: false,
              tension: 0.3,
              spanGaps: true,
            },
          ],
        }
      : null
  );

  let lineChartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: { legend: { display: true, position: 'bottom' } },
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
              label: 'Cold accuracy %',
              data: stats.categoryBreakdown.map((c) => c.coldAccuracy),
              backgroundColor: stats.categoryBreakdown.map((c) =>
                c.coldAccuracy >= 75 ? '#22c55e' : c.coldAccuracy >= 50 ? '#f59e0b' : '#ef4444'
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

  // Category breakdown sorted by cold accuracy ASC
  let sortedCategories = $derived(
    stats
      ? [...stats.categoryBreakdown].sort((a, b) => a.coldAccuracy - b.coldAccuracy)
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
        {#if srs.adaptiveWeights && srs.adaptiveWeights.length > 0}
          <div class="mt-5 pt-4 border-t border-gray-100">
            <h2 class="text-sm font-semibold text-gray-600 mb-1">Focus areas</h2>
            <p class="text-xs text-gray-400 mb-3">New clues favor your weaker categories.</p>
            <div class="flex flex-col gap-1.5">
              {#each srs.adaptiveWeights as w, i (w.category)}
                <div class="flex items-center gap-3 text-sm">
                  <span class="w-52 shrink-0 truncate text-gray-700">
                    {w.category}
                    {#if i < 3}
                      <span class="ml-1 px-1.5 py-0.5 rounded-full bg-jeopardy-gold/20 text-jeopardy-blue text-[10px] font-bold uppercase tracking-wide">Targeted</span>
                    {/if}
                  </span>
                  <div class="flex-1 h-2 bg-gray-100 rounded-full overflow-hidden">
                    <div class="h-full bg-jeopardy-blue rounded-full" style="width: {Math.round(w.weight * 100)}%"></div>
                  </div>
                  <span class="w-28 shrink-0 text-right text-xs text-gray-500">
                    {Math.round(w.accuracy)}% · {w.attempts} tries
                  </span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
        {#if srs.deck}
          <div class="mt-5 pt-4 border-t border-gray-100 flex flex-wrap gap-x-5 gap-y-1 text-sm">
            <a href="/cards?state=learning" class="text-jeopardy-blue hover:underline">
              <span class="font-bold">{srs.deck.learning}</span> learning
            </a>
            <a href="/cards?state=mastered" class="text-jeopardy-blue hover:underline">
              <span class="font-bold">{srs.deck.mastered}</span> mastered
            </a>
            <a href="/cards?state=struggling" class="{srs.deck.struggling > 0 ? 'text-red-600' : 'text-jeopardy-blue'} hover:underline">
              <span class="font-bold">{srs.deck.struggling}</span> struggling
            </a>
          </div>
        {/if}
      </div>
    {/if}

    <!-- Blind spots -->
    {#if blindspots && blindspots.configured}
      <a
        href="/blindspots"
        class="bg-white rounded-xl shadow-sm p-5 mb-8 flex items-center justify-between hover:bg-gray-50 transition-colors group block"
      >
        <div>
          <p class="font-semibold text-gray-800">Blind spots</p>
          {#if blindspots.packs.length > 0}
            <p class="text-sm text-gray-500 mt-0.5">
              {blindspots.packs.slice(0, 3).map((p) => p.theme).join(' · ')}
            </p>
          {:else if blindspots.insufficientData}
            <p class="text-sm text-gray-500 mt-0.5">Keep practicing — analysis unlocks after a few more misses.</p>
          {:else}
            <p class="text-sm text-gray-500 mt-0.5">Analyze your recent misses for patterns.</p>
          {/if}
        </div>
        <span class="text-gray-400 group-hover:text-gray-600 text-lg">&rarr;</span>
      </a>
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
        href="/drill"
        class="px-5 py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors"
      >
        Drill
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
      <!-- Cold (test-relevant) vs review stats -->
      <div class="flex flex-wrap gap-4 mb-8">
        <div class="flex-[2] min-w-[240px] bg-white rounded-xl shadow p-6 border-2 border-jeopardy-blue">
          <p class="text-sm font-medium text-gray-500 mb-1">Cold Accuracy — last 30 days</p>
          <p class="text-4xl font-extrabold {stats.cold30d.accuracy >= 70 ? 'text-green-600' : stats.cold30d.accuracy >= 55 ? 'text-amber-500' : 'text-red-500'}">
            {stats.cold30d.accuracy.toFixed(1)}%
          </p>
          <p class="text-xs text-gray-400 mt-1">
            First-attempt questions only ({stats.cold30d.total} clues) — the number the Anytime Test measures. All-time: {stats.cold.accuracy.toFixed(1)}%.
          </p>
        </div>
        <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
          <p class="text-sm font-medium text-gray-500 mb-1">Retention (review accuracy)</p>
          <p class="text-3xl font-bold text-jeopardy-blue">{stats.review.accuracy.toFixed(1)}%</p>
          <p class="text-xs text-gray-400 mt-1">{stats.review.total.toLocaleString()} SRS reviews</p>
        </div>
        <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
          <p class="text-sm font-medium text-gray-500 mb-1">Mock Test Readiness</p>
          {#if stats.mockReadiness.tests.length > 0}
            <p class="text-3xl font-bold {(stats.mockReadiness.latest ?? 0) >= stats.mockReadiness.passLine ? 'text-green-600' : 'text-jeopardy-blue'}">
              {stats.mockReadiness.latest}/50
            </p>
            <p class="text-xs text-gray-400 mt-1">Best {stats.mockReadiness.best}/50 · pass line {stats.mockReadiness.passLine} · <a href="/mock" class="text-jeopardy-blue hover:underline">take another →</a></p>
          {:else}
            <p class="text-sm text-gray-500 mt-1">No mocks yet.</p>
            <a href="/mock" class="text-sm font-semibold text-jeopardy-blue hover:underline">Take your first mock test →</a>
          {/if}
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
                  <th class="text-right py-3 px-4 font-semibold text-gray-600">Cold</th>
                  <th class="text-right py-3 px-4 font-semibold text-gray-600">Review</th>
                </tr>
              </thead>
              <tbody>
                {#each sortedCategories as cat}
                  <tr class="border-b border-gray-100 hover:bg-gray-50 transition-colors">
                    <td class="py-3 px-4 text-gray-800">{cat.category}</td>
                    <td class="py-3 px-4 text-right text-gray-600">{cat.total}</td>
                    <td class="py-3 px-4 text-right text-gray-600">{cat.correct}</td>
                    <td class="py-3 px-4 text-right font-medium {cat.coldAccuracy >= 70 ? 'text-green-600' : cat.coldAccuracy >= 50 ? 'text-amber-500' : 'text-red-500'}">
                      {cat.coldTotal > 0 ? `${cat.coldAccuracy.toFixed(1)}% (${cat.coldTotal})` : '—'}
                    </td>
                    <td class="py-3 px-4 text-right text-gray-600">
                      {cat.reviewTotal > 0 ? `${cat.reviewAccuracy.toFixed(1)}% (${cat.reviewTotal})` : '—'}
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
