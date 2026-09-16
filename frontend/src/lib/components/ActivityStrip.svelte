<script lang="ts" module>
  export interface Activity {
    streak: number;
    activeLast28: number;
    days: Array<{ date: string; active: boolean }>;
  }
</script>

<script lang="ts">
  let { activity }: { activity: Activity } = $props();

  // green ≥ 24 of 28, amber ≥ 16, red below (spec §4).
  let countClass = $derived(
    activity.activeLast28 >= 24 ? 'text-green-600' : activity.activeLast28 >= 16 ? 'text-amber-500' : 'text-red-500'
  );
  const dayLabel = (iso: string) => new Date(iso + 'T00:00:00').toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
</script>

<div class="bg-white rounded-xl shadow-sm px-5 py-3 mb-6 flex flex-wrap items-center gap-x-6 gap-y-2">
  <p class="text-sm text-gray-600">
    Streak <span class="text-xl font-bold text-jeopardy-blue">{activity.streak}</span>
    · <span class="text-xl font-bold {countClass}">{activity.activeLast28}</span> of last 28 days
  </p>
  <div class="flex gap-1 flex-wrap" aria-label="Active days, oldest to newest">
    {#each activity.days as d (d.date)}
      <span
        class="inline-block w-3 h-3 rounded-full {d.active ? 'bg-jeopardy-blue' : 'bg-gray-200'}"
        title="{dayLabel(d.date)}{d.active ? ' · active' : ''}"
      ></span>
    {/each}
  </div>
</div>
