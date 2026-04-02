<script lang="ts">
  import { Chart, registerables } from 'chart.js';
  Chart.register(...registerables);

  let { type, data, options = {} }: { type: 'line' | 'bar'; data: any; options?: any } = $props();

  let canvas: HTMLCanvasElement;
  let chart: Chart | undefined;

  $effect(() => {
    if (!canvas || !data) return;
    if (chart) chart.destroy();
    chart = new Chart(canvas, { type, data, options });
    return () => { chart?.destroy(); };
  });
</script>

<canvas bind:this={canvas}></canvas>
