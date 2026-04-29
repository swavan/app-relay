<script lang="ts">
  import { onMount } from "svelte";
  import {
    TauriConnectionProfileService,
    type ConnectionProfile
  } from "./connectionProfiles";
  import {
    UIApplicationPermissionService,
    type ApplicationPermission
  } from "./applicationPermissions";
  import { buildAppViewModel } from "./appViewModel";
  import {
    TauriRemoteService,
    type AppSummary,
    type ApplicationSession,
    type Capability,
    type HealthStatus
  } from "./services";

  const profilesService = new TauriConnectionProfileService();
  const permissionService = new UIApplicationPermissionService();
  let remote = new TauriRemoteService();

  let health: HealthStatus | null = null;
  let capabilities: Capability[] = [];
  let profiles: ConnectionProfile[] = [];
  let permissions: ApplicationPermission[] = [];
  let selectedProfile: ConnectionProfile | null = null;
  let activeSession: ApplicationSession | null = null;
  let apps: AppSummary[] = [];
  let view: "tile" | "list" = "tile";
  let errorMessage = "";
  let sessionMessage = "";
  let loading = true;

  $: appView = buildAppViewModel({
    health,
    capabilities,
    apps,
    errorMessage,
    selectedProfileLabel: selectedProfile?.label ?? null,
    activeSession,
    loading
  });

  onMount(async () => {
    try {
      loading = true;
      profiles = await profilesService.list();
      permissions = await permissionService.list();
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
    } finally {
      loading = false;
    }
  });

  async function createSession(app: AppSummary) {
    try {
      sessionMessage = "";
      if (!hasPermission(app)) {
        const granted = window.confirm(`Allow Swavan AppRelay to open ${app.name}?`);
        if (!granted) {
          sessionMessage = `Permission denied for ${app.name}`;
          return;
        }

        await permissionService.save({ applicationId: app.id, label: app.name });
        permissions = await permissionService.list();
      }

      activeSession = await remote.createSession(app.id, { width: 1280, height: 720 });
    } catch (error) {
      sessionMessage = error instanceof Error ? error.message : String(error);
    }
  }

  function hasPermission(app: AppSummary) {
    return permissions.some((permission) => permission.applicationId === app.id);
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
    <strong>{appView.healthText}</strong>
  </section>

  <section class="status" aria-label="Connection profile">
    <span>Profile</span>
    <strong>{appView.connectionLabel}</strong>
  </section>

  {#if appView.loadState === "error"}
    <section class="status error" aria-label="Connection error">
      <span>Connection error</span>
      <strong>{appView.errorText}</strong>
    </section>
  {/if}

  <section class="status" aria-label="Capabilities">
    <span>Capabilities</span>
    <strong>{appView.capabilitiesText}</strong>
  </section>

  {#if activeSession}
    <section class="status" aria-label="Application session">
      <span>{appView.sessionTitle}</span>
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
    {#if appView.loadState === "loading"}
      <p class="empty">Loading applications...</p>
    {:else if appView.loadState === "empty"}
      <p class="empty">{appView.emptyText}</p>
    {:else}
      {#each appView.apps as app}
        <button class="app" on:click={() => createSession(app)} type="button">
          {#if app.iconView.kind === "image"}
            <img class="icon" src={app.iconView.url} alt="" title={app.iconView.title} />
          {:else}
            <span class="icon" aria-hidden="true" title={app.iconView.title}>
              {app.iconView.label}
            </span>
          {/if}
          <span>{app.name}</span>
        </button>
      {/each}
    {/if}
  </section>
</main>
