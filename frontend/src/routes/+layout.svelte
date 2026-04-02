<script lang="ts">
  import '../app.css';
  import Nav from '$lib/components/Nav.svelte';
  import { checkAuth, getAuth } from '$lib/auth.svelte';
  import { page } from '$app/state';

  let { children } = $props();
  const auth = getAuth();

  const publicPaths = ['/', '/login', '/register'];

  $effect(() => {
    checkAuth();
  });
</script>

{#if auth.loading}
  <div class="min-h-screen flex items-center justify-center bg-gray-50">
    <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-jeopardy-blue"></div>
  </div>
{:else}
  {#if auth.user && !publicPaths.includes(page.url.pathname)}
    <Nav />
  {/if}
  {@render children()}
{/if}
