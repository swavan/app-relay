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
  import VideoRenderer from "./VideoRenderer.svelte";
  import {
    TauriRemoteService,
    type AppSummary,
    type ApplicationSession,
    type Capability,
    type HealthStatus,
    type InputEvent,
    type ViewportSize
  } from "./services";
  import {
    centerPoint,
    inputModeFromDelivery,
    inputViewportForSession
  } from "./inputForwarding";
  import type { VideoStreamSession } from "./videoStreams";

  const profilesService = new TauriConnectionProfileService();
  const permissionService = new UIApplicationPermissionService();
  let remote = new TauriRemoteService();

  let health: HealthStatus | null = null;
  let capabilities: Capability[] = [];
  let profiles: ConnectionProfile[] = [];
  let permissions: ApplicationPermission[] = [];
  let selectedProfile: ConnectionProfile | null = null;
  let activeSession: ApplicationSession | null = null;
  let activeStream: VideoStreamSession | null = null;
  let apps: AppSummary[] = [];
  let view: "tile" | "list" = "tile";
  let viewportWidth = "1280";
  let viewportHeight = "720";
  let errorMessage = "";
  let sessionMessage = "";
  let streamMessage = "";
  let inputMessage = "";
  let inputMode = false;
  let loading = true;

  const viewportPresets: ViewportSize[] = [
    { width: 1280, height: 720 },
    { width: 1440, height: 900 },
    { width: 1920, height: 1080 }
  ];

  $: requestedViewport = parseViewport(viewportWidth, viewportHeight);
  $: viewportValid = requestedViewport !== null;

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
      activeSession = (await remote.activeSessions())[0] ?? null;
      if (activeSession) {
        setViewport(activeSession.viewport);
      }
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
      health = {
        service: "apprelay-server",
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
        const granted = window.confirm(`Allow AppRelay to open ${app.name}?`);
        if (!granted) {
          sessionMessage = `Permission denied for ${app.name}`;
          return;
        }

        await permissionService.save({ applicationId: app.id, label: app.name });
        permissions = await permissionService.list();
      }

      if (!requestedViewport) {
        sessionMessage = "Viewport must be a whole-pixel size.";
        return;
      }

      const previousSession = activeSession;
      if (previousSession && inputMode) {
        await blurInputSession(previousSession);
      } else {
        inputMode = false;
        inputMessage = "";
      }

      if (activeStream && activeStream.state !== "stopped") {
        await remote.stopVideoStream(activeStream.id);
      }
      activeStream = null;
      activeSession = await remote.createSession(app.id, requestedViewport);
    } catch (error) {
      sessionMessage = error instanceof Error ? error.message : String(error);
    }
  }

  async function resizeSession() {
    if (!activeSession) {
      return;
    }

    if (!requestedViewport) {
      sessionMessage = "Viewport must be a whole-pixel size.";
      return;
    }

    try {
      sessionMessage = "";
      activeSession = await remote.resizeSession(activeSession.id, requestedViewport);
      if (activeStream && activeStream.state !== "stopped") {
        activeStream = await remote.videoStreamStatus(activeStream.id);
      }
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
      if (activeStream) {
        await remote.stopVideoStream(activeStream.id);
        activeStream = null;
      }
      inputMode = false;
      await remote.closeSession(activeSession.id);
      activeSession = null;
    } catch (error) {
      sessionMessage = error instanceof Error ? error.message : String(error);
    }
  }

  async function startStream() {
    if (!activeSession) {
      return;
    }

    try {
      streamMessage = "";
      activeStream = await remote.startVideoStream(activeSession.id);
    } catch (error) {
      streamMessage = error instanceof Error ? error.message : String(error);
    }
  }

  async function setInputMode(enabled: boolean) {
    if (!activeSession) {
      return;
    }

    const delivery = await forwardInput({ kind: enabled ? "focus" : "blur" });
    if (delivery) {
      inputMode = inputModeFromDelivery(inputMode, delivery);
    }
  }

  async function forwardInput(event: InputEvent) {
    if (!activeSession) {
      return null;
    }

    try {
      inputMessage = "";
      const delivery = await remote.forwardInput(
        activeSession.id,
        inputViewportForSession(activeSession),
        event
      );
      inputMessage = `Input ${delivery.status}`;
      return delivery;
    } catch (error) {
      inputMode = false;
      inputMessage = error instanceof Error ? error.message : String(error);
      return null;
    }
  }

  async function sendTestClick() {
    if (!activeSession) {
      return;
    }

    // Phase 5 test controls target the current session viewport. Real pointer
    // forwarding should switch to measured renderer bounds once decoded video exists.
    const point = centerPoint(inputViewportForSession(activeSession));
    await forwardInput({
      kind: "pointerButton",
      position: point,
      button: "primary",
      action: "press"
    });
    await forwardInput({
      kind: "pointerButton",
      position: point,
      button: "primary",
      action: "release"
    });
  }

  async function sendTestText() {
    await forwardInput({ kind: "keyboardText", text: "AppRelay" });
  }

  async function blurInputSession(session: ApplicationSession) {
    try {
      await remote.forwardInput(session.id, inputViewportForSession(session), { kind: "blur" });
    } catch {
      // Best-effort cleanup before replacing a session; server-side session auth still gates delivery.
    } finally {
      inputMode = false;
      inputMessage = "";
    }
  }

  async function stopStream() {
    if (!activeStream) {
      return;
    }

    try {
      streamMessage = "";
      activeStream = await remote.stopVideoStream(activeStream.id);
    } catch (error) {
      streamMessage = error instanceof Error ? error.message : String(error);
    }
  }

  async function reconnectStream() {
    if (!activeStream) {
      return;
    }

    try {
      streamMessage = "";
      activeStream = await remote.reconnectVideoStream(activeStream.id);
    } catch (error) {
      streamMessage = error instanceof Error ? error.message : String(error);
    }
  }

  function setViewport(viewport: ViewportSize) {
    viewportWidth = String(viewport.width);
    viewportHeight = String(viewport.height);
  }

  function parseViewport(width: string, height: string): ViewportSize | null {
    const parsedWidth = Number(width);
    const parsedHeight = Number(height);

    if (
      !Number.isInteger(parsedWidth) ||
      !Number.isInteger(parsedHeight) ||
      parsedWidth <= 0 ||
      parsedHeight <= 0
    ) {
      return null;
    }

    return {
      width: parsedWidth,
      height: parsedHeight
    };
  }

  function viewportMatches(viewport: ViewportSize) {
    return (
      requestedViewport?.width === viewport.width &&
      requestedViewport?.height === viewport.height
    );
  }
</script>

<main class="shell">
  <section class="toolbar" aria-label="Remote controls">
    <div>
      <h1>AppRelay</h1>
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
    <span>{health?.service ?? "apprelay-server"}</span>
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

  <section class="viewport-controls" aria-label="Requested viewport">
    <div class="viewport-fields">
      <label>
        <span>Width</span>
        <input
          bind:value={viewportWidth}
          inputmode="numeric"
          min="1"
          step="1"
          type="number"
        />
      </label>
      <label>
        <span>Height</span>
        <input
          bind:value={viewportHeight}
          inputmode="numeric"
          min="1"
          step="1"
          type="number"
        />
      </label>
    </div>

    <div class="presets" aria-label="Viewport presets">
      {#each viewportPresets as preset}
        <button
          class:active={viewportMatches(preset)}
          on:click={() => setViewport(preset)}
          type="button"
        >
          {preset.width} x {preset.height}
        </button>
      {/each}
    </div>
  </section>

  {#if activeSession}
    <section class="status" aria-label="Application session">
      <span>
        {appView.sessionTitle}
        <small>{activeSession.viewport.width} x {activeSession.viewport.height}</small>
      </span>
      <div class="session-actions">
        <button disabled={!viewportValid} on:click={resizeSession} type="button">Resize</button>
        {#if activeStream && activeStream.state !== "stopped"}
          <button on:click={reconnectStream} type="button">Reconnect</button>
          <button on:click={stopStream} type="button">Stop Stream</button>
        {:else}
          <button on:click={startStream} type="button">Stream</button>
        {/if}
        <button on:click={closeSession} type="button">Close</button>
      </div>
    </section>
    <section class="status" aria-label="Input forwarding">
      <span>Input</span>
      <div class="session-actions">
        <button
          class:active={inputMode}
          on:click={() => setInputMode(!inputMode)}
          type="button"
        >
          {inputMode ? "Blur" : "Focus"}
        </button>
        <button disabled={!inputMode} on:click={sendTestClick} type="button">
          Click
        </button>
        <button disabled={!inputMode} on:click={sendTestText} type="button">
          Type
        </button>
      </div>
    </section>
  {/if}

  {#if activeSession}
    <VideoRenderer stream={activeStream} requestedViewport={requestedViewport} />
  {/if}

  {#if sessionMessage}
    <section class="status error" aria-label="Session error">
      <span>Session error</span>
      <strong>{sessionMessage}</strong>
    </section>
  {/if}

  {#if streamMessage}
    <section class="status error" aria-label="Stream error">
      <span>Stream error</span>
      <strong>{streamMessage}</strong>
    </section>
  {/if}

  {#if inputMessage}
    <section class="status" aria-label="Input status">
      <span>Input status</span>
      <strong>{inputMessage}</strong>
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
          <span class="app-copy">
            <strong>{app.name}</strong>
            <small>{app.launchLabel}</small>
          </span>
        </button>
      {/each}
    {/if}
  </section>
</main>
