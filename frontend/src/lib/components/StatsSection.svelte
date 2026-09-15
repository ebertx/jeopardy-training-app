<script lang="ts">
  import type { Snippet } from 'svelte';
  import StatsChart from '$lib/components/StatsChart.svelte';
  import type {
    CategoryStat,
    DailyStat,
    DeckStats,
    ForecastDay,
    KindStat,
    SummaryStrip,
  } from '$lib/stats';
  import { localDateKey, fmtDate } from '$lib/stats';

  interface Props {
    title: string;
    accent: 'blue' | 'gold';
    summary: SummaryStrip | null;
    forecast: ForecastDay[] | null;
    deck: DeckStats | null;
    /** Link base for deck bucket labels, e.g. '/cards?state=' — omit for plain text. */
    deckLinkBase?: string;
    cold30d: KindStat | null;
    cold: KindStat | null;
    review: KindStat | null;
    /** Caption under the cold tile's number, e.g. 'clues' or 'cue cards'. */
    unitLabel: string;
    reviewLabel: string;
    /** ISO timestamp of the first logged review; used for the empty state. */
    historySince?: string | null;
    daily: DailyStat[];
    categories: CategoryStat[];
    isMobile: boolean;
    extras?: Snippet;
    afterSummary?: Snippet;
    readiness?: Snippet;
    belowTiles?: Snippet;
  }

  let {
    title,
    accent,
    summary,
    forecast,
    deck,
    deckLinkBase,
    cold30d,
    cold,
    review,
    unitLabel,
    reviewLabel,
    historySince = null,
    daily,
    categories,
    isMobile,
    extras,
    afterSummary,
    readiness,
    belowTiles,
  }: Props = $props();

  const DECK_BUCKETS = [
    { key: 'learning', label: 'learning', color: '#f59e0b' },
    { key: 'maturing', label: 'maturing', color: '#60a5fa' },
    { key: 'mastered', label: 'mastered', color: '#22c55e' },
    { key: 'struggling', label: 'struggling', color: '#ef4444' },
    { key: 'banished', label: 'banished', color: '#9ca3af' },
  ] as const;
  type BucketKey = (typeof DECK_BUCKETS)[number]['key'];
  const bucketCount = (d: DeckStats, k: BucketKey) => (k === 'banished' ? (d.banished ?? 0) : d[k]);
  const fmtDelta = (n: number) => (n > 0 ? `+${n}` : `${n}`);

  let accentBtn = $derived(
    accent === 'gold'
      ? 'bg-jeopardy-gold text-jeopardy-blue hover:bg-yellow-400'
      : 'bg-jeopardy-blue text-white hover:bg-blue-800'
  );
  let accentBar = $derived(accent === 'gold' ? '#d4b200' : '#0c47b7');

  let hasHistory = $derived((cold?.total ?? 0) + (review?.total ?? 0) > 0);

  let lineChartData = $derived(
    daily.length
      ? {
          labels: daily.map((d) => d.date),
          datasets: [
            {
              label: 'Cold (first attempt) %',
              data: daily.map((d) => (d.coldTotal > 0 ? d.coldAccuracy : null)),
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
              data: daily.map((d) => (d.reviewTotal > 0 ? d.reviewAccuracy : null)),
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

  const lineChartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: { legend: { display: true, position: 'bottom' } },
    scales: {
      y: { min: 0, max: 100, title: { display: true, text: 'Accuracy %' } },
      x: { ticks: { maxRotation: 45 } },
    },
  };

  // Due forecast (7 days on phones, 14 otherwise), padded so quiet days render
  // as true zeros; axis built from local calendar days to match the backend.
  let forecastDays = $derived(isMobile ? 7 : 14);
  let forecastChartData = $derived.by(() => {
    if (!forecast) return null;
    const counts = new Map(forecast.map((f) => [f.date, f.count]));
    const start = new Date();
    const labels: string[] = [];
    const data: number[] = [];
    for (let i = 0; i < forecastDays; i++) {
      const d = new Date(start.getFullYear(), start.getMonth(), start.getDate() + i);
      labels.push(i === 0 ? 'Today' : d.toLocaleDateString([], { weekday: 'short', day: 'numeric' }));
      data.push(counts.get(localDateKey(d)) ?? 0);
    }
    return {
      labels,
      datasets: [{ label: 'Reviews due', data, backgroundColor: accentBar, borderRadius: 4, maxBarThickness: 28 }],
    };
  });

  const forecastChartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: { legend: { display: false } },
    scales: { y: { min: 0, ticks: { precision: 0 } }, x: { grid: { display: false } } },
  };

  let barChartData = $derived(
    categories.length
      ? {
          labels: categories.map((c) => c.category),
          datasets: [
            {
              label: 'Cold accuracy %',
              data: categories.map((c) => c.coldAccuracy),
              backgroundColor: categories.map((c) =>
                c.coldAccuracy >= 75 ? '#22c55e' : c.coldAccuracy >= 50 ? '#f59e0b' : '#ef4444'
              ),
              borderWidth: 1,
            },
          ],
        }
      : null
  );

  // Phones flip to horizontal bars so category names read normally.
  let barChartOptions = $derived(
    isMobile
      ? {
          responsive: true,
          maintainAspectRatio: false,
          indexAxis: 'y' as const,
          plugins: { legend: { display: false } },
          scales: { x: { min: 0, max: 100, title: { display: true, text: 'Cold accuracy %' } } },
        }
      : {
          responsive: true,
          maintainAspectRatio: false,
          scales: {
            y: { min: 0, max: 100, title: { display: true, text: 'Accuracy %' } },
            x: { ticks: { maxRotation: 45 } },
          },
        }
  );
  let barChartHeight = $derived(isMobile ? Math.max(220, categories.length * 28 + 60) : 300);

  let sortedCategories = $derived([...categories].sort((a, b) => a.coldAccuracy - b.coldAccuracy));
</script>

<section class="mb-12">
  <h2 class="text-2xl font-bold mb-4 {accent === 'gold' ? 'text-jeopardy-blue' : 'text-gray-800'}">
    {#if accent === 'gold'}<span class="inline-block w-2 h-6 bg-jeopardy-gold rounded align-middle mr-2"></span>{/if}{title}
  </h2>

  <!-- Summary card: strip + forecast + deck + section extras -->
  {#if summary}
    <div class="bg-white rounded-xl shadow-sm p-5 mb-8">
      <div class="flex flex-wrap gap-8">
        <div>
          <p class="text-3xl font-bold text-jeopardy-blue">{summary.due}</p>
          <p class="text-xs uppercase text-gray-500">Due today</p>
        </div>
        <div>
          <p class="text-3xl font-bold text-jeopardy-blue">{summary.newLeft}</p>
          <p class="text-xs uppercase text-gray-500">New left</p>
        </div>
        <div>
          <p class="text-3xl font-bold text-jeopardy-blue">{summary.reviewedToday}</p>
          <p class="text-xs uppercase text-gray-500">Reviewed today</p>
        </div>
        <a
          href={summary.href}
          class="w-full text-center sm:w-auto sm:ml-auto self-center px-4 py-2 rounded-lg text-sm font-semibold transition-colors {accentBtn}"
        >
          {summary.label} &rarr;
        </a>
      </div>

      {#if forecastChartData}
        <div class="mt-5 pt-4 border-t border-gray-100">
          <h3 class="text-sm font-semibold text-gray-600 mb-2">Reviews due — next {forecastDays} days</h3>
          <div class="h-36">
            <StatsChart type="bar" data={forecastChartData} options={forecastChartOptions} />
          </div>
        </div>
      {/if}

      {#if extras}{@render extras()}{/if}

      {#if deck && deck.total > 0}
        <div class="mt-5 pt-4 border-t border-gray-100">
          <h3 class="text-sm font-semibold text-gray-600 mb-2">Deck · {deck.total.toLocaleString()} cards</h3>
          <div class="flex h-3 rounded-full overflow-hidden bg-gray-100">
            {#each DECK_BUCKETS as b (b.key)}
              {@const n = bucketCount(deck, b.key)}
              {#if n > 0}
                <div style="width: {(n / deck.total) * 100}%; background: {b.color}" title="{n} {b.label}"></div>
              {/if}
            {/each}
          </div>
          <div class="mt-2 flex flex-wrap gap-x-5 gap-y-1 text-sm">
            {#each DECK_BUCKETS as b (b.key)}
              {@const n = bucketCount(deck, b.key)}
              {#if b.key !== 'banished' || n > 0}
                {#if deckLinkBase && b.key !== 'banished'}
                  <a href="{deckLinkBase}{b.key}" class="text-gray-700 hover:underline">
                    <span class="inline-block w-2 h-2 rounded-full align-middle mr-1" style="background: {b.color}"></span>
                    <span class="font-bold">{n}</span> {b.label}
                  </a>
                {:else}
                  <span class="text-gray-700">
                    <span class="inline-block w-2 h-2 rounded-full align-middle mr-1" style="background: {b.color}"></span>
                    <span class="font-bold">{n}</span> {b.label}
                  </span>
                {/if}
              {/if}
            {/each}
          </div>
          {#if deck.delta}
            <p class="mt-1.5 text-xs text-gray-400">
              Since {fmtDate(deck.delta.since)}:
              mastered {fmtDelta(deck.delta.mastered)} · struggling {fmtDelta(deck.delta.struggling)}
            </p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if afterSummary}{@render afterSummary()}{/if}

  <!-- Accuracy tiles + readiness slot -->
  {#if cold30d && cold && review}
    <div class="flex flex-wrap gap-4 mb-8">
      <div class="flex-[2] min-w-[240px] bg-white rounded-xl shadow p-6 border-2 {accent === 'gold' ? 'border-jeopardy-gold' : 'border-jeopardy-blue'}">
        <p class="text-sm font-medium text-gray-500 mb-1">Cold Accuracy — last 30 days</p>
        {#if hasHistory}
          <p class="text-4xl font-extrabold {cold30d.accuracy >= 70 ? 'text-green-600' : cold30d.accuracy >= 55 ? 'text-amber-500' : 'text-red-500'}">
            {cold30d.total > 0 ? `${cold30d.accuracy.toFixed(1)}%` : '—'}
          </p>
          <p class="text-xs text-gray-400 mt-1">
            First-attempt {unitLabel} only ({cold30d.total}) — the number the Anytime Test measures. All-time: {cold.accuracy.toFixed(1)}%.
          </p>
        {:else}
          <p class="text-4xl font-extrabold text-gray-300">—</p>
          <p class="text-xs text-gray-400 mt-1">
            {historySince ? `Tracking since ${fmtDate(historySince.slice(0, 10))}.` : 'Tracking starts with your next drill.'}
          </p>
        {/if}
      </div>
      <div class="flex-1 min-w-[200px] bg-white rounded-xl shadow p-6">
        <p class="text-sm font-medium text-gray-500 mb-1">Retention (review accuracy)</p>
        {#if hasHistory && review.total > 0}
          <p class="text-3xl font-bold text-jeopardy-blue">{review.accuracy.toFixed(1)}%</p>
          <p class="text-xs text-gray-400 mt-1">{review.total.toLocaleString()} {reviewLabel}</p>
        {:else}
          <p class="text-3xl font-bold text-gray-300">—</p>
          <p class="text-xs text-gray-400 mt-1">No reviews logged yet.</p>
        {/if}
      </div>
      {#if readiness}{@render readiness()}{/if}
    </div>
  {/if}

  {#if belowTiles}{@render belowTiles()}{/if}

  <!-- Accuracy chart -->
  {#if lineChartData}
    <div class="bg-white rounded-xl shadow p-6 mb-8">
      <h3 class="text-lg font-semibold text-gray-800 mb-4">Accuracy — last 30 days</h3>
      <div style="height: 300px;">
        <StatsChart type="line" data={lineChartData} options={lineChartOptions} />
      </div>
    </div>
  {:else}
    <div class="bg-white rounded-xl shadow p-6 mb-8 text-center text-gray-400">No daily performance data yet.</div>
  {/if}

  <!-- Category chart -->
  {#if barChartData}
    <div class="bg-white rounded-xl shadow p-6 mb-8">
      <h3 class="text-lg font-semibold text-gray-800 mb-4">Category Performance</h3>
      <div style="height: {barChartHeight}px;">
        <StatsChart type="bar" data={barChartData} options={barChartOptions} />
      </div>
    </div>
  {:else}
    <div class="bg-white rounded-xl shadow p-6 mb-8 text-center text-gray-400">No category data yet.</div>
  {/if}

  <!-- Category table -->
  {#if sortedCategories.length > 0}
    <div class="bg-white rounded-xl shadow p-6">
      <h3 class="text-lg font-semibold text-gray-800 mb-4">Category Breakdown</h3>
      <div class="overflow-x-auto">
        <table class="min-w-full text-sm">
          <thead>
            <tr class="border-b border-gray-200">
              <th class="text-left py-3 px-2 sm:px-4 font-semibold text-gray-600">Category</th>
              <th class="hidden sm:table-cell text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Total</th>
              <th class="hidden sm:table-cell text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Correct</th>
              <th class="text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Cold</th>
              <th class="text-right py-3 px-2 sm:px-4 font-semibold text-gray-600">Review</th>
            </tr>
          </thead>
          <tbody>
            {#each sortedCategories as cat (cat.category)}
              <tr class="border-b border-gray-100 hover:bg-gray-50 transition-colors">
                <td class="py-3 px-2 sm:px-4 text-gray-800">{cat.category}</td>
                <td class="hidden sm:table-cell py-3 px-2 sm:px-4 text-right text-gray-600">{cat.total}</td>
                <td class="hidden sm:table-cell py-3 px-2 sm:px-4 text-right text-gray-600">{cat.correct}</td>
                <td class="py-3 px-2 sm:px-4 text-right font-medium {cat.coldAccuracy >= 70 ? 'text-green-600' : cat.coldAccuracy >= 50 ? 'text-amber-500' : 'text-red-500'}">
                  {cat.coldTotal > 0 ? `${cat.coldAccuracy.toFixed(1)}% (${cat.coldTotal})` : '—'}
                </td>
                <td class="py-3 px-2 sm:px-4 text-right text-gray-600">
                  {cat.reviewTotal > 0 ? `${cat.reviewAccuracy.toFixed(1)}% (${cat.reviewTotal})` : '—'}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {/if}
</section>
