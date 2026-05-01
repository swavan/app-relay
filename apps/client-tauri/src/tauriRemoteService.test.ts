import { beforeEach, describe, expect, it, vi } from "vitest";
import { TauriRemoteService } from "./services";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn()
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke
}));

describe("TauriRemoteService", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
  });

  it("sends the configured profile token and client id through the control-plane path", async () => {
    tauri.invoke
      .mockResolvedValueOnce({
        service: "apprelay-server",
        healthy: true,
        version: "mobile-contract-test"
      })
      .mockResolvedValueOnce([
        {
          platform: "linux",
          feature: "appDiscovery",
          supported: true
        }
      ])
      .mockResolvedValueOnce([
        {
          id: "terminal",
          name: "Terminal"
        }
      ])
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([]);

    const service = new TauriRemoteService("mobile-test-token", "mobile-test-server");

    await expect(service.health()).resolves.toMatchObject({
      service: "apprelay-server",
      healthy: true
    });
    await expect(service.capabilities()).resolves.toHaveLength(1);
    await expect(service.applications()).resolves.toEqual([{ id: "terminal", name: "Terminal" }]);
    await expect(service.activeSessions()).resolves.toEqual([]);
    await expect(service.activeInputFocus()).resolves.toBeNull();
    await expect(service.activeVideoStreams()).resolves.toEqual([]);
    await expect(service.activeAudioStreams()).resolves.toEqual([]);

    expect(tauri.invoke.mock.calls).toEqual([
      ["server_health", { authToken: "mobile-test-token" }],
      ["server_capabilities", { authToken: "mobile-test-token" }],
      ["server_applications", { authToken: "mobile-test-token" }],
      [
        "active_application_sessions",
        { authToken: "mobile-test-token", clientId: "mobile-test-server" }
      ],
      [
        "active_input_focus",
        { authToken: "mobile-test-token", clientId: "mobile-test-server" }
      ],
      [
        "active_video_streams",
        { authToken: "mobile-test-token", clientId: "mobile-test-server" }
      ],
      [
        "active_audio_streams",
        { authToken: "mobile-test-token", clientId: "mobile-test-server" }
      ]
    ]);
  });

  it("passes client id on sensitive session, input, and stream commands", async () => {
    tauri.invoke
      .mockResolvedValueOnce({
        id: "session-1",
        applicationId: "terminal",
        selectedWindow: {
          id: "window-session-1",
          applicationId: "terminal",
          title: "Terminal",
          selectionMethod: "synthetic"
        },
        viewport: { width: 1280, height: 720 },
        state: "ready"
      })
      .mockResolvedValueOnce({
        id: "session-1",
        applicationId: "terminal",
        selectedWindow: {
          id: "window-session-1",
          applicationId: "terminal",
          title: "Terminal",
          selectionMethod: "synthetic"
        },
        viewport: { width: 1440, height: 900 },
        state: "ready"
      })
      .mockResolvedValueOnce({
        sessionId: "session-1",
        selectedWindowId: "window-session-1",
        mappedEvent: { kind: "focus" },
        status: "focused"
      })
      .mockResolvedValueOnce({
        sessionId: "session-1",
        selectedWindowId: "window-session-1"
      })
      .mockResolvedValueOnce({
        id: "stream-1",
        sessionId: "session-1",
        selectedWindowId: "window-session-1",
        viewport: { width: 1280, height: 720 },
        state: "starting"
      })
      .mockResolvedValueOnce({
        id: "audio-stream-1",
        sessionId: "session-1",
        selectedWindowId: "window-session-1",
        state: "starting"
      })
      .mockResolvedValueOnce({
        id: "session-1",
        applicationId: "terminal",
        selectedWindow: {
          id: "window-session-1",
          applicationId: "terminal",
          title: "Terminal",
          selectionMethod: "synthetic"
        },
        viewport: { width: 1440, height: 900 },
        state: "closed"
      });

    const service = new TauriRemoteService("mobile-test-token", "mobile-test-server");

    await service.createSession("terminal", { width: 1280, height: 720 });
    await service.resizeSession("session-1", { width: 1440, height: 900 });
    await service.forwardInput("session-1", { width: 1440, height: 900 }, { kind: "focus" });
    await service.activeInputFocus();
    await service.startVideoStream("session-1");
    await service.startAudioStream("session-1", {
      outputDeviceId: "default",
      inputDeviceId: "microphone",
      microphone: "enabled",
      systemAudioMuted: false,
      microphoneMuted: true
    });
    await service.closeSession("session-1");

    expect(tauri.invoke.mock.calls).toEqual([
      [
        "create_application_session",
        {
          authToken: "mobile-test-token",
          clientId: "mobile-test-server",
          request: {
            applicationId: "terminal",
            viewport: { width: 1280, height: 720 }
          }
        }
      ],
      [
        "resize_application_session",
        {
          authToken: "mobile-test-token",
          clientId: "mobile-test-server",
          request: {
            sessionId: "session-1",
            viewport: { width: 1440, height: 900 }
          }
        }
      ],
      [
        "forward_input",
        {
          authToken: "mobile-test-token",
          clientId: "mobile-test-server",
          request: {
            sessionId: "session-1",
            clientViewport: { width: 1440, height: 900 },
            event: { kind: "focus" }
          }
        }
      ],
      [
        "active_input_focus",
        {
          authToken: "mobile-test-token",
          clientId: "mobile-test-server"
        }
      ],
      [
        "start_video_stream",
        {
          authToken: "mobile-test-token",
          clientId: "mobile-test-server",
          request: { sessionId: "session-1" }
        }
      ],
      [
        "start_audio_stream",
        {
          authToken: "mobile-test-token",
          clientId: "mobile-test-server",
          request: {
            sessionId: "session-1",
            outputDeviceId: "default",
            inputDeviceId: "microphone",
            microphone: "enabled",
            systemAudioMuted: false,
            microphoneMuted: true
          }
        }
      ],
      [
        "close_application_session",
        {
          authToken: "mobile-test-token",
          clientId: "mobile-test-server",
          sessionId: "session-1"
        }
      ]
    ]);
  });
});
