<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    onclose,
    closeOnOverlay = true,
    closeOnEscape = true,
    ariaLabel,
    ariaLabelledby,
    children,
  }: {
    onclose: () => void;
    closeOnOverlay?: boolean;
    closeOnEscape?: boolean;
    ariaLabel?: string;
    ariaLabelledby?: string;
    children: Snippet;
  } = $props();

  let dialogEl = $state<HTMLDivElement | null>(null);

  const FOCUSABLE = [
    'button:not([disabled])',
    '[href]',
    'input:not([disabled])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    '[tabindex]:not([tabindex="-1"])',
  ].join(',');

  $effect(() => {
    const previousActive = document.activeElement as HTMLElement | null;
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';

    queueMicrotask(() => {
      const first = dialogEl?.querySelector<HTMLElement>(FOCUSABLE);
      first?.focus();
    });

    return () => {
      document.body.style.overflow = prevOverflow;
      previousActive?.focus?.();
    };
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && closeOnEscape) {
      e.preventDefault();
      onclose();
      return;
    }
    if (e.key === 'Tab' && dialogEl) {
      const focusables = Array.from(dialogEl.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (focusables.length === 0) {
        e.preventDefault();
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement as HTMLElement | null;
      const inDialog = active && dialogEl.contains(active);
      if (e.shiftKey && (!inDialog || active === first)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (!inDialog || active === last)) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4"
  onclick={(e) => {
    if (closeOnOverlay && e.target === e.currentTarget) onclose();
  }}
  role="presentation"
>
  <div
    bind:this={dialogEl}
    role="dialog"
    aria-modal="true"
    aria-label={ariaLabel}
    aria-labelledby={ariaLabelledby}
    tabindex="-1"
    class="outline-none w-full max-w-sm"
  >
    {@render children()}
  </div>
</div>
