<script lang="ts">
  import './app.css';
  import AppShell from './lib/AppShell.svelte';
  import OperatorDesk from './OperatorDesk.svelte';
</script>

<svelte:head>
  <title>Shea Symphony App</title>
  <meta
    name="description"
    content="A high-fidelity local foreground workflow cockpit for Shea Symphony."
  />
</svelte:head>

<AppShell>
  <OperatorDesk />
</AppShell>
