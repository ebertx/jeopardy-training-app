<script lang="ts">
  let {
    categories,
    selected,
    onchange,
  }: {
    categories: Array<{ name: string; count: number }>;
    selected: string;
    onchange: (value: string) => void;
  } = $props();
</script>

<select
  value={selected}
  onchange={(e) => onchange((e.target as HTMLSelectElement).value)}
  class="block w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-800 shadow-sm focus:border-jeopardy-blue focus:outline-none focus:ring-1 focus:ring-jeopardy-blue"
>
  <option value="all">All Categories</option>
  {#each categories as cat}
    <option value={cat.name}>{cat.name} ({cat.count})</option>
  {/each}
</select>
