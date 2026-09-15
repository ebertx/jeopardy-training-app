<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { onMount } from 'svelte';
  import StatsChart from '$lib/components/StatsChart.svelte';

  interface MockRow {
    id: number;
    completedAt: string;
    score: number | null;
    missKinds: { unknown: number; slow: number; wording: number };
  }
  interface History { tests: MockRow[]; best: number | null; passLine: number }

  const auth = getAuth();
  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });

  let history = $state<History | null>(null);
  let loading = $state(true);
  let error = $state('');

  onMount(async () => {
    try {
      history = await api.get('/api/mock-test/history');
    } catch (err: any) {
      error = err?.message ?? 'Failed to load mock history';
    } finally {
      loading = false;
    }
  });

  const fmtDate = (iso: string) =>
    new Date(iso).toLocaleDateString([], { year: 'numeric', month: 'short', day: 'numeric' });

  // Newest first from the API; the chart wants oldest → newest.
  let chronological = $derived(history ? [...history.tests].reverse() : []);

  /** Score change versus the previous (older) test, or null for the first one. */
  const delta = (row: MockRow): number | null => {
    if (!history || row.score === null) return null;
    const idx = chronological.findIndex((t) => t.id === row.id);
    const prev = idx > 0 ? chronological[idx - 1].score : null;
    return prev === null ? null : row.score - prev;
  };

  let chartData = $derived(
    history && chronological.length > 0
      ? {
          labels: chronological.map((t) => fmtDate(t.completedAt)),
          datasets: [
            {
              label: 'Score',
              data: chronological.map((t) => t.score),
              borderColor: '#0c47b7',
              borderWidth: 2.5,
              pointRadius: 4,
              pointBackgroundColor: '#0c47b7',
              fill: false,
              tension: 0.2,
            },
            {
              label: `Pass line (${history.passLine})`,
              data: chronological.map(() => history!.passLine),
              borderColor: '#22c55e',
              borderWidth: 1.5,
              borderDash: [6, 4],
              pointRadius: 0,
              fill: false,
            },
          ],
        }
      : null
  );
  const chartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: { legend: { display: true, position: 'bottom' } },
    scales: { y: { min: 0, max: 50, ticks: { stepSize: 10 } } },
  };
</script>

<svelte:head>
  <title>Mock Test History — Jeopardy! Training</title>
</svelte:head>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-4xl mx-auto">
    <div class="flex flex-wrap items-center justify-between gap-3 mb-6">
      <h1 class="text-3xl font-bold text-jeopardy-blue">Mock Test History</h1>
      <a href="/mock" class="px-4 py-2 rounded-lg bg-jeopardy-blue text-white text-sm font-semibold hover:bg-blue-800 transition-colors">Take a mock &rarr;</a>
    </div>

    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg">{error}</div>
    {:else if history && history.tests.length === 0}
      <div class="bg-white rounded-xl shadow p-8 text-center text-gray-500">
        No completed mock tests yet. <a href="/mock" class="text-jeopardy-blue hover:underline">Take your first one &rarr;</a>
      </div>
    {:else if history}
      <div class="flex flex-wrap gap-6 mb-6 text-sm text-gray-500">
        <span><span class="text-2xl font-bold text-jeopardy-blue">{history.tests.length}</span> tests</span>
        <span><span class="text-2xl font-bold text-jeopardy-blue">{history.best ?? '—'}</span>/50 best</span>
        <span><span class="text-2xl font-bold text-jeopardy-blue">{history.passLine}</span> pass line</span>
      </div>

      {#if chartData}
        <div class="bg-white rounded-xl shadow p-6 mb-6">
          <h2 class="text-lg font-semibold text-gray-800 mb-4">Score over time</h2>
          <div style="height: 260px;">
            <StatsChart type="line" data={chartData} options={chartOptions} />
          </div>
        </div>
      {/if}

      <div class="bg-white rounded-xl shadow p-6">
        <div class="overflow-x-auto">
          <table class="min-w-full text-sm">
            <thead>
              <tr class="border-b border-gray-200">
                <th class="text-left py-3 px-2 sm:px-4 font-semibold text-gray-600">Date</th>
                <th class="text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Score</th>
                <th class="text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Change</th>
                <th class="hidden sm:table-cell text-right py-3 px-2 sm:px-4 font-semibold text-gray-600" title="Misses tagged unknown / slow / wording">Miss tags</th>
                <th class="py-3 px-2 sm:px-4"></th>
              </tr>
            </thead>
            <tbody>
              {#each history.tests as t (t.id)}
                {@const d = delta(t)}
                {@const tagged = t.missKinds.unknown + t.missKinds.slow + t.missKinds.wording}
                <tr class="border-b border-gray-100 hover:bg-gray-50 transition-colors">
                  <td class="py-3 px-2 sm:px-4 text-gray-800">{fmtDate(t.completedAt)}</td>
                  <td class="py-3 px-2 sm:px-4 text-right font-bold {t.score !== null && t.score >= history.passLine ? 'text-green-600' : 'text-jeopardy-blue'}">
                    {t.score ?? '—'}<span class="text-gray-400 font-normal">/50</span>
                  </td>
                  <td class="py-3 px-2 sm:px-4 text-right {d === null ? 'text-gray-400' : d > 0 ? 'text-green-600' : d < 0 ? 'text-red-500' : 'text-gray-500'}">
                    {d === null ? '—' : d > 0 ? `+${d}` : `${d}`}
                  </td>
                  <td class="hidden sm:table-cell py-3 px-2 sm:px-4 text-right text-gray-600">
                    {#if tagged > 0}
                      {t.missKinds.unknown} unknown · {t.missKinds.slow} slow · {t.missKinds.wording} wording
                    {:else}
                      <span class="text-gray-400">untagged</span>
                    {/if}
                  </td>
                  <td class="py-3 px-2 sm:px-4 text-right">
                    <a href="/mock?results={t.id}" class="text-jeopardy-blue hover:underline whitespace-nowrap">Results &rarr;</a>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </div>
    {/if}
  </div>
</div>
