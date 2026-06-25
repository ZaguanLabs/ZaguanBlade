<script>
  export let title = "Dashboard";

  export function openPanel() {
    return title;
  }
</script>

<main id="svelte-fixture" class="svelteLayout dense-grid">
  <h1>{title}</h1>
</main>

<style>
.svelte-action {
  --svelte-gap: 1rem;
}
</style>
