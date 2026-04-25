<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && auth.user) {
      goto('/dashboard');
    }
  });
</script>

<div class="min-h-screen bg-jeopardy-blue flex items-center justify-center px-4">
  <div class="text-center">
    <h1 class="text-jeopardy-gold text-5xl font-bold mb-4">Jeopardy! Training</h1>
    <p class="text-white text-xl mb-10">Master your trivia skills with 500,000+ questions</p>
    <div class="flex flex-col sm:flex-row gap-4 justify-center">
      <a
        href="/login"
        class="px-8 py-3 bg-jeopardy-gold text-jeopardy-blue font-bold rounded-lg text-lg hover:bg-yellow-400 transition-colors"
      >
        Login
      </a>
      <a
        href="/register"
        class="px-8 py-3 border-2 border-jeopardy-gold text-jeopardy-gold font-bold rounded-lg text-lg hover:bg-jeopardy-gold hover:text-jeopardy-blue transition-colors"
      >
        Register
      </a>
    </div>
  </div>
</div>
