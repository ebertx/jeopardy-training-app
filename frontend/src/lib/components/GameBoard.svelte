<script lang="ts">
  interface Question {
    col: number;
    row: number;
    question_id: number | null;
    value: number;
    answered: string | null;
    daily_double: boolean;
  }

  let {
    categories,
    questions,
    onselect,
  }: {
    categories: string[];
    questions: Question[];
    onselect: (col: number, row: number) => void;
  } = $props();

  function getQuestion(col: number, row: number): Question | undefined {
    return questions.find((q) => q.col === col && q.row === row);
  }

  function formatValue(value: number): string {
    return '$' + value.toLocaleString();
  }
</script>

<div class="w-full overflow-x-auto">
  <div
    class="grid gap-1 min-w-[480px]"
    style="grid-template-columns: repeat(6, 1fr);"
  >
    <!-- Category headers -->
    {#each categories as category, i}
      <div
        class="bg-jeopardy-blue text-white text-center font-bold text-xs sm:text-sm py-3 px-2 rounded flex items-center justify-center min-h-[64px]"
        style="grid-column: {i + 1};"
      >
        {category}
      </div>
    {/each}

    <!-- Value rows (5 rows) -->
    {#each [1, 2, 3, 4, 5] as row}
      {#each [0, 1, 2, 3, 4, 5] as col}
        {@const q = getQuestion(col, row)}
        {#if q === undefined || q.question_id === null}
          <div
            class="bg-gray-300 text-gray-500 text-center font-bold text-lg py-4 rounded flex items-center justify-center min-h-[56px] cursor-not-allowed"
          >
            —
          </div>
        {:else if q.answered !== null}
          <div
            class="bg-gray-400 text-gray-200 text-center font-bold text-lg py-4 rounded flex items-center justify-center min-h-[56px] cursor-not-allowed"
          >
          </div>
        {:else}
          <button
            class="relative bg-jeopardy-blue hover:bg-blue-700 active:bg-blue-800 text-yellow-300 text-center font-bold text-lg py-4 rounded flex items-center justify-center min-h-[56px] cursor-pointer transition-colors w-full"
            onclick={() => onselect(col, row)}
          >
            {formatValue(q.value)}
            {#if q.daily_double}
              <span class="absolute top-1 right-1 bg-red-500 text-white text-[10px] font-bold px-1 py-0.5 rounded leading-none">DD</span>
            {/if}
          </button>
        {/if}
      {/each}
    {/each}
  </div>
</div>
