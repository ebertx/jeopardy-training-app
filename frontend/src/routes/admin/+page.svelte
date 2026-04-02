<script lang="ts">
  import { onMount } from 'svelte';
  import { getAuth } from '$lib/auth.svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';

  const auth = getAuth();

  $effect(() => {
    if (!auth.loading) {
      if (!auth.user) goto('/login');
      else if (auth.user.role !== 'admin') goto('/dashboard');
    }
  });

  interface UserRecord {
    id: number;
    username: string;
    email: string;
    role: string;
    approved: boolean;
    created_at: string;
  }

  let users = $state<UserRecord[]>([]);
  let loading = $state(true);
  let error = $state('');
  let approving = $state<Set<number>>(new Set());

  function formatDate(dateStr: string): string {
    return new Date(dateStr).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  }

  async function fetchUsers() {
    loading = true;
    error = '';
    try {
      users = await api.get('/api/admin/users');
    } catch (err: any) {
      error = err?.message ?? 'Failed to load users';
    } finally {
      loading = false;
    }
  }

  async function approveUser(userId: number) {
    const next = new Set(approving);
    next.add(userId);
    approving = next;

    try {
      await api.post('/api/admin/approve', { userId });
      // Update local state
      users = users.map((u) => (u.id === userId ? { ...u, approved: true } : u));
    } catch (err: any) {
      error = err?.message ?? 'Failed to approve user';
    } finally {
      const next2 = new Set(approving);
      next2.delete(userId);
      approving = next2;
    }
  }

  onMount(fetchUsers);
</script>

<div class="min-h-screen bg-gray-50 py-8 px-4">
  <div class="max-w-5xl mx-auto flex flex-col gap-6">

    <div class="flex items-center justify-between">
      <h1 class="text-3xl font-bold text-jeopardy-blue">Admin Panel</h1>
      <a href="/settings" class="text-sm text-gray-500 hover:underline">&larr; Back to Settings</a>
    </div>

    {#if error}
      <div class="px-4 py-3 bg-red-50 border border-red-200 text-red-700 rounded-lg text-sm">
        {error}
        <button onclick={() => (error = '')} class="ml-2 underline">Dismiss</button>
      </div>
    {/if}

    {#if loading}
      <div class="flex justify-center py-16">
        <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-jeopardy-blue"></div>
      </div>
    {:else}
      <div class="bg-white rounded-xl shadow overflow-hidden">
        <div class="px-6 py-4 border-b border-gray-100">
          <h2 class="text-lg font-semibold text-gray-800">Users ({users.length})</h2>
        </div>
        <div class="overflow-x-auto">
          <table class="min-w-full text-sm">
            <thead class="bg-gray-50 border-b border-gray-200">
              <tr>
                <th class="text-left py-3 px-4 font-semibold text-gray-600">Username</th>
                <th class="text-left py-3 px-4 font-semibold text-gray-600">Email</th>
                <th class="text-left py-3 px-4 font-semibold text-gray-600">Role</th>
                <th class="text-left py-3 px-4 font-semibold text-gray-600">Approved</th>
                <th class="text-left py-3 px-4 font-semibold text-gray-600">Created</th>
                <th class="text-left py-3 px-4 font-semibold text-gray-600">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each users as user, i}
                <tr class="border-b border-gray-100 {i % 2 === 0 ? '' : 'bg-gray-50'} hover:bg-blue-50 transition-colors">
                  <td class="py-3 px-4 font-medium text-gray-800">{user.username}</td>
                  <td class="py-3 px-4 text-gray-600">{user.email}</td>
                  <td class="py-3 px-4">
                    <span class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium
                      {user.role === 'admin' ? 'bg-purple-100 text-purple-800' : 'bg-blue-100 text-blue-800'}">
                      {user.role}
                    </span>
                  </td>
                  <td class="py-3 px-4">
                    {#if user.approved}
                      <span class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-green-100 text-green-800">
                        Yes
                      </span>
                    {:else}
                      <span class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium bg-red-100 text-red-800">
                        No
                      </span>
                    {/if}
                  </td>
                  <td class="py-3 px-4 text-gray-600">{formatDate(user.created_at)}</td>
                  <td class="py-3 px-4">
                    {#if !user.approved}
                      <button
                        onclick={() => approveUser(user.id)}
                        disabled={approving.has(user.id)}
                        class="px-3 py-1.5 bg-green-600 hover:bg-green-700 disabled:opacity-60 text-white text-xs font-semibold rounded-lg transition-colors"
                      >
                        {approving.has(user.id) ? 'Approving...' : 'Approve'}
                      </button>
                    {:else}
                      <span class="text-gray-400 text-xs">—</span>
                    {/if}
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
