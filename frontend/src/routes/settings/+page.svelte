<script lang="ts">
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { logout } from '$lib/auth.svelte';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading && !auth.user) goto('/login');
  });
</script>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-lg mx-auto flex flex-col gap-6">

    <h1 class="text-3xl font-bold text-jeopardy-blue">Settings</h1>

    {#if auth.loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else if auth.user}
      <!-- Profile Card -->
      <div class="bg-white rounded-xl shadow p-6 flex flex-col gap-4">
        <h2 class="text-lg font-semibold text-gray-800 border-b border-gray-100 pb-3">Profile</h2>

        <div class="flex flex-col gap-3">
          <div class="flex flex-col gap-1">
            <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide">Username</p>
            <p class="text-gray-800 font-medium">{auth.user.username}</p>
          </div>
          <div class="flex flex-col gap-1">
            <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide">Email</p>
            <p class="text-gray-800 font-medium">{auth.user.email}</p>
          </div>
          <div class="flex flex-col gap-1">
            <p class="text-xs font-semibold text-gray-500 uppercase tracking-wide">Role</p>
            <span class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium w-fit
              {auth.user.role === 'admin' ? 'bg-purple-100 text-purple-800' : 'bg-blue-100 text-blue-800'}">
              {auth.user.role}
            </span>
          </div>
        </div>
      </div>

      <!-- Admin Link -->
      {#if auth.user.role === 'admin'}
        <a
          href="/admin"
          class="bg-white rounded-xl shadow p-5 flex items-center justify-between hover:bg-gray-50 transition-colors group"
        >
          <div>
            <p class="font-semibold text-gray-800">Admin Panel</p>
            <p class="text-sm text-gray-500 mt-0.5">Manage users and approvals</p>
          </div>
          <span class="text-gray-400 group-hover:text-gray-600 text-lg">&rarr;</span>
        </a>
      {/if}

      <!-- Sign Out -->
      <div class="bg-white rounded-xl shadow p-6">
        <h2 class="text-lg font-semibold text-gray-800 mb-4">Account</h2>
        <button
          onclick={logout}
          class="px-5 py-2.5 border border-red-300 text-red-600 font-semibold rounded-lg hover:bg-red-50 transition-colors text-sm"
        >
          Sign Out
        </button>
      </div>
    {/if}

  </div>
</div>
