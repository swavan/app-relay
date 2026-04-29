import { describe, expect, it } from "vitest";
import type { RemoteService, ViewportSize } from "./services";
import type { WebRtcIceCandidate, WebRtcSessionDescription } from "./videoStreams";

const streamOffer = {
  sdpType: "offer" as const,
  sdp: "apprelay-webrtc-offer:stream-1:window-session-1"
};

const serverIceCandidate = {
  candidate: "candidate:apprelay stream-1 window-session-1 typ host",
  sdpMid: "video",
  sdpMLineIndex: 0
};

class FakeRemoteService implements RemoteService {
  async health() {
    return {
      service: "apprelay-server",
      healthy: true,
      version: "test"
    };
  }

  async capabilities() {
    return [
      {
        platform: "linux",
        feature: "appDiscovery",
        supported: true
      }
    ];
  }

  async applications() {
    return [
      {
        id: "terminal",
        name: "Terminal",
        icon: {
          mimeType: "application/x-icon-theme-name",
          dataUrl: undefined,
          source: "utilities-terminal"
        },
        launch: {
          kind: "desktopCommand" as const,
          value: "gnome-terminal"
        }
      }
    ];
  }

  async activeSessions() {
    return [
      {
        id: "session-1",
        applicationId: "terminal",
        selectedWindow: {
          id: "window-session-1",
          applicationId: "terminal",
          selectionMethod: "launchIntent" as const,
          title: "Terminal"
        },
        viewport: {
          width: 1280,
          height: 720
        },
        state: "ready" as const
      }
    ];
  }

  async createSession(applicationId: string, viewport: ViewportSize) {
    return {
      id: "session-1",
      applicationId,
      selectedWindow: {
        id: "window-session-1",
        applicationId,
        selectionMethod: "launchIntent" as const,
        title: applicationId
      },
      launchIntent: {
        sessionId: "session-1",
        applicationId,
        launch: {
          kind: "desktopCommand" as const,
          value: "gnome-terminal"
        },
        status: "recorded" as const
      },
      viewport,
      state: "ready" as const
    };
  }

  async resizeSession(sessionId: string, viewport: ViewportSize) {
    return {
      id: sessionId,
      applicationId: "terminal",
      selectedWindow: {
        id: "window-session-1",
        applicationId: "terminal",
        selectionMethod: "synthetic" as const,
        title: "terminal"
      },
      viewport,
      resizeIntent: {
        sessionId,
        selectedWindowId: "window-session-1",
        viewport,
        status: "recorded" as const
      },
      state: "ready" as const
    };
  }

  async closeSession(sessionId: string) {
    return {
      id: sessionId,
      applicationId: "terminal",
      selectedWindow: {
        id: "window-session-1",
        applicationId: "terminal",
        selectionMethod: "synthetic" as const,
        title: "terminal"
      },
      viewport: {
        width: 1280,
        height: 720
      },
      state: "closed" as const
    };
  }

  async startVideoStream(sessionId: string) {
    return {
      id: "stream-1",
      sessionId,
      selectedWindowId: "window-session-1",
      viewport: {
        width: 1280,
        height: 720
      },
      captureSource: {
        scope: "selectedWindow" as const,
        selectedWindowId: "window-session-1",
        applicationId: "terminal",
        title: "Terminal"
      },
      signaling: {
        kind: "webRtcOffer" as const,
        negotiationState: "awaitingAnswer" as const,
        offer: streamOffer,
        iceCandidates: [serverIceCandidate]
      },
      stats: {
        framesEncoded: 0,
        bitrateKbps: 0,
        latencyMs: 0,
        reconnectAttempts: 0
      },
      health: {
        healthy: true
      },
      state: "starting" as const
    };
  }

  async stopVideoStream(streamId: string) {
    return {
      id: streamId,
      sessionId: "session-1",
      selectedWindowId: "window-session-1",
      viewport: {
        width: 1280,
        height: 720
      },
      captureSource: {
        scope: "selectedWindow" as const,
        selectedWindowId: "window-session-1",
        applicationId: "terminal",
        title: "Terminal"
      },
      signaling: {
        kind: "webRtcOffer" as const,
        negotiationState: "awaitingAnswer" as const,
        offer: streamOffer,
        iceCandidates: [serverIceCandidate]
      },
      stats: {
        framesEncoded: 0,
        bitrateKbps: 0,
        latencyMs: 0,
        reconnectAttempts: 0
      },
      health: {
        healthy: false,
        message: "stream stopped by client"
      },
      state: "stopped" as const
    };
  }

  async reconnectVideoStream(streamId: string) {
    return {
      id: streamId,
      sessionId: "session-1",
      selectedWindowId: "window-session-1",
      viewport: {
        width: 1280,
        height: 720
      },
      captureSource: {
        scope: "selectedWindow" as const,
        selectedWindowId: "window-session-1",
        applicationId: "terminal",
        title: "Terminal"
      },
      signaling: {
        kind: "webRtcOffer" as const,
        negotiationState: "awaitingAnswer" as const,
        offer: streamOffer,
        iceCandidates: [serverIceCandidate]
      },
      stats: {
        framesEncoded: 0,
        bitrateKbps: 0,
        latencyMs: 0,
        reconnectAttempts: 1
      },
      health: {
        healthy: true,
        message: "reconnect requested"
      },
      state: "starting" as const
    };
  }

  async negotiateVideoStream(
    streamId: string,
    clientAnswer: WebRtcSessionDescription,
    clientIceCandidates: WebRtcIceCandidate[]
  ) {
    return {
      id: streamId,
      sessionId: "session-1",
      selectedWindowId: "window-session-1",
      viewport: {
        width: 1280,
        height: 720
      },
      captureSource: {
        scope: "selectedWindow" as const,
        selectedWindowId: "window-session-1",
        applicationId: "terminal",
        title: "Terminal"
      },
      signaling: {
        kind: "webRtcOffer" as const,
        negotiationState: "negotiated" as const,
        offer: streamOffer,
        answer: clientAnswer,
        iceCandidates: [serverIceCandidate, ...clientIceCandidates]
      },
      stats: {
        framesEncoded: 0,
        bitrateKbps: 0,
        latencyMs: 0,
        reconnectAttempts: 0
      },
      health: {
        healthy: true,
        message: "WebRTC negotiation completed"
      },
      state: "streaming" as const
    };
  }

  async videoStreamStatus(streamId: string) {
    return {
      id: streamId,
      sessionId: "session-1",
      selectedWindowId: "window-session-1",
      viewport: {
        width: 1280,
        height: 720
      },
      captureSource: {
        scope: "selectedWindow" as const,
        selectedWindowId: "window-session-1",
        applicationId: "terminal",
        title: "Terminal"
      },
      signaling: {
        kind: "webRtcOffer" as const,
        negotiationState: "awaitingAnswer" as const,
        offer: streamOffer,
        iceCandidates: [serverIceCandidate]
      },
      stats: {
        framesEncoded: 0,
        bitrateKbps: 0,
        latencyMs: 0,
        reconnectAttempts: 0
      },
      health: {
        healthy: true
      },
      state: "starting" as const
    };
  }
}

describe("RemoteService contract", () => {
  it("returns server health", async () => {
    const service = new FakeRemoteService();

    await expect(service.health()).resolves.toEqual({
      service: "apprelay-server",
      healthy: true,
      version: "test"
    });
  });

  it("returns capabilities and applications", async () => {
    const service = new FakeRemoteService();

    await expect(service.capabilities()).resolves.toHaveLength(1);
    await expect(service.applications()).resolves.toEqual([
      {
        id: "terminal",
        name: "Terminal",
        icon: {
          mimeType: "application/x-icon-theme-name",
          dataUrl: undefined,
          source: "utilities-terminal"
        },
        launch: {
          kind: "desktopCommand",
          value: "gnome-terminal"
        }
      }
    ]);
  });

  it("creates, resizes, and closes sessions", async () => {
    const service = new FakeRemoteService();

    await expect(service.activeSessions()).resolves.toEqual([
      {
        id: "session-1",
        applicationId: "terminal",
        selectedWindow: {
          id: "window-session-1",
          applicationId: "terminal",
          selectionMethod: "launchIntent",
          title: "Terminal"
        },
        viewport: {
          width: 1280,
          height: 720
        },
        state: "ready"
      }
    ]);
    await expect(service.createSession("terminal", { width: 1280, height: 720 })).resolves.toMatchObject({
      id: "session-1",
      applicationId: "terminal",
      viewport: { width: 1280, height: 720 },
      selectedWindow: {
        applicationId: "terminal",
        selectionMethod: "launchIntent"
      },
      launchIntent: {
        applicationId: "terminal",
        status: "recorded"
      },
      state: "ready"
    });
    await expect(service.resizeSession("session-1", { width: 1440, height: 900 })).resolves.toMatchObject({
      id: "session-1",
      viewport: { width: 1440, height: 900 },
      resizeIntent: {
        selectedWindowId: "window-session-1",
        status: "recorded"
      }
    });
    await expect(service.closeSession("session-1")).resolves.toMatchObject({
      id: "session-1",
      state: "closed"
    });
  });

  it("starts, checks, and stops video streams", async () => {
    const service = new FakeRemoteService();

    await expect(service.startVideoStream("session-1")).resolves.toMatchObject({
      id: "stream-1",
      sessionId: "session-1",
      selectedWindowId: "window-session-1",
      captureSource: {
        scope: "selectedWindow"
      },
      signaling: {
        kind: "webRtcOffer",
        negotiationState: "awaitingAnswer",
        offer: {
          sdpType: "offer"
        },
        iceCandidates: [
          {
            sdpMid: "video"
          }
        ]
      },
      state: "starting"
    });
    await expect(
      service.negotiateVideoStream(
        "stream-1",
        { sdpType: "answer", sdp: "client-answer" },
        [{ candidate: "candidate:client stream-1 typ host", sdpMid: "video", sdpMLineIndex: 0 }]
      )
    ).resolves.toMatchObject({
      id: "stream-1",
      signaling: {
        negotiationState: "negotiated",
        answer: {
          sdpType: "answer",
          sdp: "client-answer"
        }
      },
      health: {
        message: "WebRTC negotiation completed"
      },
      state: "streaming"
    });
    await expect(service.videoStreamStatus("stream-1")).resolves.toMatchObject({
      id: "stream-1",
      state: "starting"
    });
    await expect(service.reconnectVideoStream("stream-1")).resolves.toMatchObject({
      id: "stream-1",
      stats: {
        reconnectAttempts: 1
      },
      health: {
        message: "reconnect requested"
      },
      state: "starting"
    });
    await expect(service.stopVideoStream("stream-1")).resolves.toMatchObject({
      id: "stream-1",
      state: "stopped"
    });
  });
});
