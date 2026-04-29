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
        service: "swavan-server",
        healthy: true,
        version: "0.1.0",
      },
      capabilities: [
        {
          platform: "linux",
          feature: "appDiscovery",
          supported: true,
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
    expect(viewModel.sessionTitle).toBe("Terminal");
    expect(viewModel.apps).toEqual([
      {
        id: "terminal",
        name: "Terminal",
        iconView: {
          kind: "label",
          label: "TE",
          title: "Terminal",
        },
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
});
