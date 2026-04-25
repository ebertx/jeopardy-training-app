<script lang="ts">
  import { getAuth, logout } from '$lib/auth.svelte';
  import { page } from '$app/state';

  const auth = getAuth();
  let menuOpen = $state(false);

  const links = [
    { href: '/quiz', label: 'Quiz' },
    { href: '/coryat', label: 'Coryat' },
    { href: '/review', label: 'Review' },
    { href: '/mastered', label: 'Mastered' },
    { href: '/study', label: 'Study' },
    { href: '/dashboard', label: 'Dashboard' },
    { href: '/settings', label: 'Settings' },
  ];

  function isActive(href: string): boolean {
    return page.url.pathname === href;
  }

  function toggleMenu() {
    menuOpen = !menuOpen;
  }

  async function handleLogout() {
    await logout();
  }
</script>

<nav class="bg-jeopardy-blue text-jeopardy-gold shadow-md">
  <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
    <div class="flex items-center justify-between h-14">
      <!-- App title -->
      <a href="/dashboard" class="font-bold text-lg text-jeopardy-gold hover:text-white transition-colors">
        Jeopardy! Training
      </a>

      <!-- Desktop links -->
      <div class="hidden md:flex items-center gap-1">
        {#each links as link}
          <a
            href={link.href}
            class="px-3 py-1 rounded text-sm font-medium transition-colors {isActive(link.href)
              ? 'text-white underline underline-offset-4'
              : 'text-jeopardy-gold hover:text-white'}"
          >
            {link.label}
          </a>
        {/each}

        {#if auth.user?.role === 'admin'}
          <a
            href="/admin"
            class="px-3 py-1 rounded text-sm font-medium transition-colors {isActive('/admin')
              ? 'text-white underline underline-offset-4'
              : 'text-jeopardy-gold hover:text-white'}"
          >
            Admin
          </a>
        {/if}

        <button
          onclick={handleLogout}
          class="ml-2 px-3 py-1 rounded text-sm font-medium border border-jeopardy-gold text-jeopardy-gold hover:bg-jeopardy-gold hover:text-jeopardy-blue transition-colors"
        >
          Logout
        </button>
      </div>

      <!-- Mobile hamburger -->
      <button
        class="md:hidden p-2 rounded text-jeopardy-gold hover:text-white"
        onclick={toggleMenu}
        aria-label="Toggle menu"
      >
        {#if menuOpen}
          <!-- X icon -->
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
          </svg>
        {:else}
          <!-- Hamburger icon -->
          <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6h16M4 12h16M4 18h16" />
          </svg>
        {/if}
      </button>
    </div>
  </div>

  <!-- Mobile dropdown -->
  {#if menuOpen}
    <div class="md:hidden border-t border-blue-700 px-4 py-2 flex flex-col gap-1">
      {#each links as link}
        <a
          href={link.href}
          onclick={() => (menuOpen = false)}
          class="block px-3 py-2 rounded text-sm font-medium transition-colors {isActive(link.href)
            ? 'text-white bg-blue-800'
            : 'text-jeopardy-gold hover:text-white hover:bg-blue-800'}"
        >
          {link.label}
        </a>
      {/each}

      {#if auth.user?.role === 'admin'}
        <a
          href="/admin"
          onclick={() => (menuOpen = false)}
          class="block px-3 py-2 rounded text-sm font-medium transition-colors {isActive('/admin')
            ? 'text-white bg-blue-800'
            : 'text-jeopardy-gold hover:text-white hover:bg-blue-800'}"
        >
          Admin
        </a>
      {/if}

      <button
        onclick={handleLogout}
        class="mt-1 px-3 py-2 rounded text-sm font-medium text-left border border-jeopardy-gold text-jeopardy-gold hover:bg-jeopardy-gold hover:text-jeopardy-blue transition-colors"
      >
        Logout
      </button>
    </div>
  {/if}
</nav>
