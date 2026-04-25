<script lang="ts">
  import { register } from '$lib/auth.svelte';
  import { ApiError } from '$lib/api';

  let username = $state('');
  let email = $state('');
  let password = $state('');
  let error = $state('');
  let success = $state(false);
  let submitting = $state(false);

  async function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    error = '';
    submitting = true;
    try {
      await register(username, email, password);
      success = true;
    } catch (err) {
      if (err instanceof ApiError) {
        error = err.message;
      } else {
        error = 'An unexpected error occurred.';
      }
    } finally {
      submitting = false;
    }
  }
</script>

<div class="min-h-screen bg-gray-50 flex items-center justify-center px-4">
  <div class="bg-white rounded-2xl shadow-lg w-full max-w-md p-8">
    <div class="text-center mb-8">
      <h1 class="text-3xl font-bold text-jeopardy-blue">Jeopardy! Training</h1>
      <p class="text-gray-500 mt-1">Create a new account</p>
    </div>

    {#if success}
      <div class="text-center py-6">
        <div class="mb-4 px-4 py-3 bg-green-50 border border-green-200 text-green-700 rounded-lg">
          Registration successful. Awaiting admin approval.
        </div>
        <a href="/login" class="text-jeopardy-blue font-medium hover:underline">Back to Login</a>
      </div>
    {:else}
      {#if error}
        <div class="mb-4 px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
          {error}
        </div>
      {/if}

      <form onsubmit={handleSubmit} class="space-y-5">
        <div>
          <label for="username" class="block text-sm font-medium text-gray-700 mb-1">Username</label>
          <input
            id="username"
            type="text"
            bind:value={username}
            required
            autocomplete="username"
            class="w-full px-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-jeopardy-blue focus:border-transparent"
            placeholder="jeopardy_fan"
          />
        </div>

        <div>
          <label for="email" class="block text-sm font-medium text-gray-700 mb-1">Email</label>
          <input
            id="email"
            type="email"
            bind:value={email}
            required
            autocomplete="email"
            class="w-full px-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-jeopardy-blue focus:border-transparent"
            placeholder="you@example.com"
          />
        </div>

        <div>
          <label for="password" class="block text-sm font-medium text-gray-700 mb-1">Password</label>
          <input
            id="password"
            type="password"
            bind:value={password}
            required
            autocomplete="new-password"
            class="w-full px-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-jeopardy-blue focus:border-transparent"
            placeholder="••••••••"
          />
        </div>

        <button
          type="submit"
          disabled={submitting}
          class="w-full py-2.5 bg-jeopardy-blue text-white font-semibold rounded-lg hover:bg-blue-800 transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
        >
          {submitting ? 'Creating account…' : 'Create Account'}
        </button>
      </form>

      <p class="mt-6 text-center text-sm text-gray-500">
        Already have an account?
        <a href="/login" class="text-jeopardy-blue font-medium hover:underline">Sign in</a>
      </p>
    {/if}
  </div>
</div>
