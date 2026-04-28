<script lang="ts">
  import { onMount } from "svelte";
  import {
    TauriConnectionProfileService,
    type ConnectionProfile
  } from "./connectionProfiles";
  import {
    TauriRemoteService,
    type AppSummary,
    type ApplicationSession,
    type Capability,
    type HealthStatus
  } from "./services";

  const profilesService = new TauriConnectionProfileService();
  let remote = new TauriRemoteService();

  let health: HealthStatus | null = null;
  let capabilities: Capability[] = [];
  let profiles: ConnectionProfile[] = [];
  let selectedProfile: ConnectionProfile | null = null;
  let activeSession: ApplicationSession | null = null;
  let apps: AppSummary[] = [];
  let view: "tile" | "list" = "tile";
  let errorMessage = "";
  let sessionMessage = "";

  onMount(async () => {
    try {
      profiles = await profilesService.list();
      selectedProfile = profiles[0] ?? null;

      remote = new TauriRemoteService(selectedProfile?.authToken);
      health = await remote.health();
      capabilities = await remote.capabilities();
      apps = await remote.applications();
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
      health = {
        service: "swavan-server",
        healthy: false,
        version: "unconnected"
      };
    }
  });

  async function createSession(app: AppSummary) {
    try {
      sessionMessage = "";
      activeSession = await remote.createSession(app.id, { width: 1280, height: 720 });
    } catch (error) {
      sessionMessage = error instanceof Error ? error.message : String(error);
    }
  }

  async function closeSession() {
    if (!activeSession) {
      return;
    }

    try {
      sessionMessage = "";
      await remote.closeSession(activeSession.id);
      activeSession = null;
    } catch (error) {
      sessionMessage = error instanceof Error ? error.message : String(error);
    }
  }
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

  <section class="status" aria-label="Connection profile">
    <span>Profile</span>
    <strong>{selectedProfile?.label ?? "Local development"}</strong>
  </section>

  {#if errorMessage}
    <section class="status error" aria-label="Connection error">
      <span>Connection error</span>
      <strong>{errorMessage}</strong>
    </section>
  {/if}

  <section class="status" aria-label="Capabilities">
    <span>Capabilities</span>
    <strong>{capabilities.filter((capability) => capability.supported).length}/{capabilities.length}</strong>
  </section>

  {#if activeSession}
    <section class="status" aria-label="Application session">
      <span>{activeSession.selectedWindow.title}</span>
      <button on:click={closeSession} type="button">Close</button>
    </section>
  {/if}

  {#if sessionMessage}
    <section class="status error" aria-label="Session error">
      <span>Session error</span>
      <strong>{sessionMessage}</strong>
    </section>
  {/if}

  <section class:grid={view === "tile"} class:list={view === "list"} aria-label="Applications">
    {#if apps.length === 0}
      <p class="empty">No applications found for this server.</p>
    {:else}
      {#each apps as app}
        <button class="app" on:click={() => createSession(app)} type="button">
          <span class="icon" aria-hidden="true"></span>
          <span>{app.name}</span>
        </button>
      {/each}
    {/if}
  </section>
</main>
