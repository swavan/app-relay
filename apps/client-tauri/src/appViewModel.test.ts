import { describe, expect, it } from "vitest";
import { buildAppViewModel } from "./appViewModel";
import type { AppViewModelInput } from "./appViewModel";

const baseInput: AppViewModelInput = {
  health: null,
  capabilities: [],
  apps: [],
  errorMessage: "",
  selectedProfileLabel: null,
  activeSession: null,
  loading: false,
};

describe("buildAppViewModel", () => {
  it("returns loading state", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      loading: true,
    });

    expect(viewModel.loadState).toBe("loading");
    expect(viewModel.healthText).toBe("checking");
  });

  it("returns empty state", () => {
    const viewModel = buildAppViewModel(baseInput);

    expect(viewModel.loadState).toBe("empty");
    expect(viewModel.emptyText).toBe("No applications found for this server.");
  });

  it("returns error state", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      errorMessage: "unauthorized",
    });

    expect(viewModel.loadState).toBe("error");
    expect(viewModel.errorText).toBe("unauthorized");
  });

  it("returns success state with profile, health, capabilities, apps, and session", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      health: {
        service: "apprelay-server",
        healthy: true,
        version: "0.1.0",
      },
      capabilities: [
        {
          platform: "linux",
          feature: "appDiscovery",
          supported: true,
          reason: "Available through the server adapter.",
        },
        {
          platform: "linux",
          feature: "windowResize",
          supported: false,
        },
      ],
      apps: [
        {
          id: "terminal",
          name: "Terminal",
          launch: {
            kind: "desktopCommand",
            value: "/usr/bin/gnome-terminal --new-window",
          },
        },
      ],
      selectedProfileLabel: "Linux PC",
      activeSession: {
        id: "session-1",
        applicationId: "terminal",
        selectedWindow: {
          id: "window-1",
          applicationId: "terminal",
          title: "Terminal",
          selectionMethod: "launchIntent",
        },
        viewport: {
          width: 1280,
          height: 720,
        },
        state: "ready",
      },
    });

    expect(viewModel.loadState).toBe("success");
    expect(viewModel.connectionLabel).toBe("Linux PC");
    expect(viewModel.healthText).toBe("0.1.0");
    expect(viewModel.capabilitiesText).toBe("1/2");
    expect(viewModel.capabilityNotes).toEqual([
      {
        feature: "App Discovery",
        platform: "Linux",
        reason: "Available through the server adapter.",
      },
    ]);
    expect(viewModel.unsupportedCapabilities).toEqual([
      {
        feature: "Window Resize",
        platform: "Linux",
        reason: "No reason provided by server.",
      },
    ]);
    expect(viewModel.sessionTitle).toBe("Terminal");
    expect(viewModel.apps).toEqual([
      {
        id: "terminal",
        name: "Terminal",
        launch: {
          kind: "desktopCommand",
          value: "/usr/bin/gnome-terminal --new-window",
        },
        launchLabel: "Launch gnome-terminal",
        iconView: {
          kind: "label",
          label: "TE",
          title: "Terminal",
        },
      },
    ]);
  });

  it("surfaces supported capability notes with trimmed server reasons", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      capabilities: [
        {
          platform: "windows_desktop",
          feature: "keyboard-input",
          supported: true,
          reason: "  Routed by the active server profile.  ",
        },
      ],
    });

    expect(viewModel.capabilityNotes).toEqual([
      {
        feature: "Keyboard Input",
        platform: "Windows Desktop",
        reason: "Routed by the active server profile.",
      },
    ]);
  });

  it("preserves macOS video capability telemetry notes without implying playback", () => {
    const reason =
      "macOS selected-window video stream control-plane startup and capture runtime telemetry are available; decoded/live ScreenCaptureKit video delivery remains planned";
    const viewModel = buildAppViewModel({
      ...baseInput,
      capabilities: [
        {
          platform: "macos",
          feature: "windowVideoStream",
          supported: true,
          reason,
        },
      ],
    });

    expect(viewModel.capabilityNotes).toEqual([
      {
        feature: "Window Video Stream",
        platform: "macOS",
        reason,
      },
    ]);
  });

  it("omits unsupported capabilities and blank reasons from capability notes", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      capabilities: [
        {
          platform: "macos",
          feature: "windowVideo",
          supported: false,
          reason: "Native video capture is unavailable.",
        },
        {
          platform: "linux",
          feature: "mouseInput",
          supported: true,
          reason: " ",
        },
        {
          platform: "ios",
          feature: "audio-streaming",
          supported: true,
          reason: "",
        },
      ],
    });

    expect(viewModel.capabilitiesText).toBe("2/3");
    expect(viewModel.capabilityNotes).toEqual([]);
    expect(viewModel.unsupportedCapabilities).toEqual([
      {
        feature: "Window Video",
        platform: "macOS",
        reason: "Native video capture is unavailable.",
      },
    ]);
  });

  it("preserves unsupported capability fallback reasons", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      capabilities: [
        {
          platform: "windows_desktop",
          feature: "keyboard-input",
          supported: false,
          reason: " ",
        },
      ],
    });

    expect(viewModel.capabilitiesText).toBe("0/1");
    expect(viewModel.capabilityNotes).toEqual([]);
    expect(viewModel.unsupportedCapabilities).toEqual([
      {
        feature: "Keyboard Input",
        platform: "Windows Desktop",
        reason: "No reason provided by server.",
      },
    ]);
  });

  it("uses icon image data when available", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      apps: [
        {
          id: "terminal",
          name: "Terminal",
          icon: {
            mimeType: "image/png",
            dataUrl: "data:image/png;base64,iVBORw==",
            source: "terminal.png",
          },
        },
      ],
    });

    expect(viewModel.apps[0].iconView).toEqual({
      kind: "image",
      url: "data:image/png;base64,iVBORw==",
      label: "Terminal",
      title: "terminal.png",
    });
  });

  it("renders normalized macOS icon PNGs from the bridge", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      apps: [
        {
          id: "com.apple.Terminal",
          name: "Terminal",
          icon: {
            mimeType: "image/png",
            dataUrl: "data:image/png;base64,iVBORw0KGgo=",
            source: "Contents/Resources/Terminal.icns",
          },
        },
      ],
    });

    expect(viewModel.apps[0].iconView).toEqual({
      kind: "image",
      url: "data:image/png;base64,iVBORw0KGgo=",
      label: "Terminal",
      title: "Contents/Resources/Terminal.icns",
    });
  });

  it("uses fallback labels for non-web image icon data", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      apps: [
        {
          id: "terminal",
          name: "Terminal",
          icon: {
            mimeType: "image/icns",
            dataUrl: "data:image/icns;base64,aWNucw==",
            source: "Contents/Resources/Terminal.icns",
          },
        },
      ],
    });

    expect(viewModel.apps[0].iconView).toEqual({
      kind: "label",
      label: "TE",
      title: "Contents/Resources/Terminal.icns",
    });
  });

  it("labels apps without launch metadata as attach targets", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      apps: [
        {
          id: "terminal",
          name: "Terminal",
        },
      ],
    });

    expect(viewModel.apps[0].launchLabel).toBe("Attach to running app");
  });

  it("labels macOS bundle launch metadata", () => {
    const viewModel = buildAppViewModel({
      ...baseInput,
      apps: [
        {
          id: "com.example.Terminal",
          name: "Terminal",
          launch: {
            kind: "macosBundle",
            value: "/Applications/Terminal.app",
          },
        },
      ],
    });

    expect(viewModel.apps[0].launchLabel).toBe("Launch macOS app");
  });
});
