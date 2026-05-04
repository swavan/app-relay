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
    hydrateActiveAudioStream,
    hydrateActiveVideoStream,
    type AppSummary,
    type ApplicationSession,
    type Capability,
    type HealthStatus,
    type InputEvent,
    type ViewportSize
  } from "./services";
  import {
    centerPoint,
    inputControlAvailability,
    inputModeFromDelivery,
    inputViewportForSession
  } from "./inputForwarding";
  import type { AudioStreamSession } from "./audioStreams";
  import type { VideoStreamSession } from "./videoStreams";
  import {
    WebRtcVideoSession,
    type WebRtcVideoSessionState
  } from "./webrtcVideoSession";

  const profilesService = new TauriConnectionProfileService();
  const permissionService = new UIApplicationPermissionService();
  let remote = new TauriRemoteService();
  let webrtcSession: WebRtcVideoSession | null = null;
  let webrtcState: WebRtcVideoSessionState = { kind: "idle" };

  let health: HealthStatus | null = null;
  let capabilities: Capability[] = [];
  let profiles: ConnectionProfile[] = [];
  let permissions: ApplicationPermission[] = [];
  let selectedProfile: ConnectionProfile | null = null;
  let activeSession: ApplicationSession | null = null;
  let activeStream: VideoStreamSession | null = null;
  let activeAudioStream: AudioStreamSession | null = null;
  let apps: AppSummary[] = [];
  let view: "tile" | "list" = "tile";
  let viewportWidth = "1280";
  let viewportHeight = "720";
  let errorMessage = "";
  let sessionMessage = "";
  let streamMessage = "";
  let audioMessage = "";
  let inputMessage = "";
  let inputMode = false;
  let microphoneEnabled = false;
  let systemAudioMuted = false;
  let microphoneMuted = true;
  let outputDeviceId = "";
  let inputDeviceId = "";
  let loading = true;

  const viewportPresets: ViewportSize[] = [
    { width: 1280, height: 720 },
    { width: 1440, height: 900 },
    { width: 1920, height: 1080 }
  ];

  $: requestedViewport = parseViewport(viewportWidth, viewportHeight);
  $: viewportValid = requestedViewport !== null;
  $: audioStreamActive = activeAudioStream !== null && activeAudioStream.state !== "stopped";
  $: inputControls = inputControlAvailability(activeSession !== null, capabilities);

  $: appView = buildAppViewModel({
    health,
    capabilities,
    apps,
    errorMessage,
    selectedProfileLabel: selectedProfile?.label ?? null,
    activeSession,
    loading
  });

  $: if (webrtcSession) {
    if (activeStream) {
      webrtcSession.attach(activeStream);
    } else {
      webrtcSession.detach();
    }
  }

  $: if (webrtcState.kind === "failed") {
    streamMessage = webrtcState.error;
  }

  function rebuildWebrtcSession() {
    if (webrtcSession) {
      webrtcSession.detach();
    }
    webrtcSession = new WebRtcVideoSession({
      services: remote,
      onStateChange: (state) => {
        webrtcState = state;
      }
    });
  }

  onMount(async () => {
    try {
      loading = true;
      profiles = await profilesService.list();
      permissions = await permissionService.list();
      selectedProfile = profiles[0] ?? null;

      remote = new TauriRemoteService(selectedProfile?.authToken, selectedProfile?.id);
      rebuildWebrtcSession();
      health = await remote.health();
      capabilities = await remote.capabilities();
      apps = await remote.applications();
      const activeSessions = await remote.activeSessions();
      let focusedSessionId: string | null = null;
      let focusedSelectedWindowId: string | null = null;
      try {
        const activeFocus = await remote.activeInputFocus();
        focusedSessionId = activeFocus?.sessionId ?? null;
        focusedSelectedWindowId = activeFocus?.selectedWindowId ?? null;
      } catch (error) {
        inputMessage = error instanceof Error ? error.message : String(error);
      }
      activeSession =
        activeSessions.find(
          (session) =>
            session.id === focusedSessionId &&
            session.selectedWindow.id === focusedSelectedWindowId
        ) ??
        activeSessions[0] ??
        null;
      inputMode =
        activeSession !== null &&
        activeSession.id === focusedSessionId &&
        activeSession.selectedWindow.id === focusedSelectedWindowId;
      activeStream = null;
      if (activeSession) {
        setViewport(activeSession.viewport);
        activeStream = await hydrateActiveVideoStream(remote, activeSession.id, (message) => {
          streamMessage = message;
        });
        activeAudioStream = await hydrateActiveAudioStream(remote, activeSession.id, (message) => {
          audioMessage = message;
        });
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
      if (audioStreamActive && activeAudioStream) {
        await remote.stopAudioStream(activeAudioStream.id);
      }
      activeStream = null;
      activeAudioStream = null;
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
      if (audioStreamActive && activeAudioStream) {
        await remote.stopAudioStream(activeAudioStream.id);
      }
      activeAudioStream = null;
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
    if (!activeSession || !inputControls.testClickAvailable) {
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
    if (!inputControls.testTextAvailable) {
      return;
    }

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

  async function startAudioStream() {
    if (!activeSession) {
      return;
    }

    try {
      audioMessage = "";
      activeAudioStream = await remote.startAudioStream(activeSession.id, {
        microphone: microphoneEnabled ? "enabled" : "disabled",
        systemAudioMuted,
        microphoneMuted,
        outputDeviceId: optionalDeviceId(outputDeviceId),
        inputDeviceId: optionalDeviceId(inputDeviceId)
      });
    } catch (error) {
      audioMessage = error instanceof Error ? error.message : String(error);
    }
  }

  async function stopAudioStream() {
    if (!audioStreamActive || !activeAudioStream) {
      activeAudioStream = null;
      return;
    }

    try {
      audioMessage = "";
      const stoppedStream = await remote.stopAudioStream(activeAudioStream.id);
      activeAudioStream = stoppedStream.state === "stopped" ? null : stoppedStream;
    } catch (error) {
      audioMessage = error instanceof Error ? error.message : String(error);
    }
  }

  async function updateAudioStream() {
    if (!audioStreamActive || !activeAudioStream) {
      return;
    }

    try {
      audioMessage = "";
      activeAudioStream = await remote.updateAudioStream(activeAudioStream.id, {
        systemAudioMuted,
        microphoneMuted,
        outputDeviceId: optionalDeviceId(outputDeviceId),
        inputDeviceId: optionalDeviceId(inputDeviceId)
      });
    } catch (error) {
      audioMessage = error instanceof Error ? error.message : String(error);
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

  function optionalDeviceId(value: string) {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : undefined;
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
    <div class="capability-summary">
      <strong>{appView.capabilitiesText}</strong>
      {#if appView.capabilityNotes.length > 0}
        <ul class="capability-notes" aria-label="Capability notes">
          {#each appView.capabilityNotes as capability}
            <li>
              <span>{capability.feature}</span>
              <small>{capability.platform}: {capability.reason}</small>
            </li>
          {/each}
        </ul>
      {/if}
      {#if appView.unsupportedCapabilities.length > 0}
        <ul class="unsupported-capabilities" aria-label="Unsupported capabilities">
          {#each appView.unsupportedCapabilities as capability}
            <li>
              <span>{capability.feature}</span>
              <small>{capability.platform}: {capability.reason}</small>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
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
          disabled={!inputControls.focusAvailable}
          on:click={() => setInputMode(!inputMode)}
          type="button"
        >
          {inputMode ? "Blur" : "Focus"}
        </button>
        <button
          disabled={!inputMode || !inputControls.testClickAvailable}
          on:click={sendTestClick}
          type="button"
        >
          Click
        </button>
        <button
          disabled={!inputMode || !inputControls.testTextAvailable}
          on:click={sendTestText}
          type="button"
        >
          Type
        </button>
      </div>
    </section>
    <section class="status audio-controls" aria-label="Audio controls">
      <span>
        Audio
        {#if activeAudioStream}
          <small>{activeAudioStream.state}</small>
        {/if}
      </span>
      <div class="audio-panel">
        <label>
          <input bind:checked={microphoneEnabled} disabled={audioStreamActive} type="checkbox" />
          Mic
        </label>
        <label>
          <input bind:checked={systemAudioMuted} type="checkbox" />
          Mute audio
        </label>
        <label>
          <input bind:checked={microphoneMuted} type="checkbox" />
          Mute mic
        </label>
        <input bind:value={outputDeviceId} placeholder="Output device" type="text" />
        <input bind:value={inputDeviceId} placeholder="Input device" type="text" />
        <div class="session-actions">
          {#if audioStreamActive}
            <button on:click={updateAudioStream} type="button">Apply</button>
            <button on:click={stopAudioStream} type="button">Stop Audio</button>
          {:else}
            <button on:click={startAudioStream} type="button">Audio</button>
          {/if}
        </div>
      </div>
    </section>
  {/if}

  {#if activeSession}
    <VideoRenderer
      stream={activeStream}
      requestedViewport={requestedViewport}
      mediaStream={webrtcState.kind === "connected" ? webrtcState.mediaStream : null}
    />
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

  {#if audioMessage}
    <section class="status error" aria-label="Audio error">
      <span>Audio error</span>
      <strong>{audioMessage}</strong>
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
