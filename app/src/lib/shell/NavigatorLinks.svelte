<script lang="ts">
  import type { NavigatorItem, NavigatorNavigateHandler } from './NavigatorTypes.ts';

  export let items: NavigatorItem[] = [];
  export let currentPath = '/';
  export let onNavigate: NavigatorNavigateHandler = () => {};

  function isActivePath(href: string) {
    if (href === '/') return currentPath === '/';
    if (href === '/doctor') return currentPath === '/doctor' || currentPath.startsWith('/doctor/') || currentPath === '/observability';
    if (href === '/intelligence') return currentPath === '/intelligence' || currentPath.startsWith('/intelligence/') || currentPath === '/reference';
    return currentPath === href || currentPath.startsWith(`${href}/`);
  }
</script>

<nav class="nav-list" aria-label="Current surface">
  {#each items as item}
    <a
      class:active={isActivePath(item.href)}
      href={item.href}
      onclick={(event) => onNavigate(event, item.href)}
    >
      {item.label}
    </a>
  {/each}
</nav>
