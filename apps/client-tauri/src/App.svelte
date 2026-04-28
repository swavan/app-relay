<script lang="ts">
  import { onMount } from "svelte";
  import { UnsupportedRemoteService, type AppSummary, type HealthStatus } from "./services";

  const remote = new UnsupportedRemoteService();

  let health: HealthStatus | null = null;
  let apps: AppSummary[] = [];
  let view: "tile" | "list" = "tile";

  onMount(async () => {
    health = await remote.health();
    apps = await remote.applications();
  });
</script>

<main class="shell">
  <section class="toolbar" aria-label="Remote controls">
    <div>
      <h1>Swavan AppRelay</h1>
      <p>{health?.healthy ? "Connected" : "No server connection"}</p>
    </div>

    <div class="actions">
      <button class:active={view === "tile"} on:click={() => (view = "tile")} type="button">
        Tile
      </button>
      <button class:active={view === "list"} on:click={() => (view = "list")} type="button">
        List
      </button>
    </div>
  </section>

  <section class="status" aria-label="Server health">
    <span>{health?.service ?? "swavan-server"}</span>
    <strong>{health?.version ?? "checking"}</strong>
  </section>

  <section class:grid={view === "tile"} class:list={view === "list"} aria-label="Applications">
    {#if apps.length === 0}
      <p class="empty">Application discovery is not implemented in Phase 1.</p>
    {:else}
      {#each apps as app}
        <button class="app" type="button">
          <span class="icon" aria-hidden="true"></span>
          <span>{app.name}</span>
        </button>
      {/each}
    {/if}
  </section>
</main>
