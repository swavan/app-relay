import { describe, expect, it } from "vitest";
import type { InputEvent, RemoteService, ViewportSize } from "./services";
import type { AudioStreamStartOptions, AudioStreamUpdate } from "./audioStreams";
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

const configuredEncoding = {
  contract: {
    codec: "h264" as const,
    pixelFormat: "rgba" as const,
    hardwareAcceleration: "none" as const,
    target: {
      resolution: {
        width: 1280,
        height: 720
      },
      maxFps: 30,
      targetBitrateKbps: 2764,
      keyframeIntervalFrames: 60
    },
    adaptation: {
      requestedViewport: {
        width: 1280,
        height: 720
      },
      currentTarget: {
        width: 1280,
        height: 720
      },
      limits: {
        maxWidth: 1920,
        maxHeight: 1080,
        maxPixels: 2073600
      },
      reason: "matchesViewport" as const
    }
  },
  state: "configured" as const,
  output: {
    framesSubmitted: 0,
    framesEncoded: 0,
    keyframesEncoded: 0,
    bytesProduced: 0,
    lastFrame: null
  }
};

const nativeAudioBackendStatuses = [
  "capture",
  "playback",
  "clientMicrophoneCapture",
  "serverMicrophoneInjection"
] as const;

const pipeWireAudioBackendStatuses = nativeAudioBackendStatuses.map((leg) => ({
  leg,
  backend: "pipeWire" as const,
  available: false,
  readiness: "plannedNative" as const,
  failure: {
    kind: "nativeBackendNotImplemented" as const,
    message: `${leg} via PipeWire is not implemented yet`,
    recovery:
      "keep the control-plane stream active for state negotiation, but do not expect audio packets until the native backend is implemented"
  }
}));

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

  async forwardInput(sessionId: string, clientViewport: ViewportSize, event: InputEvent) {
    if (event.kind === "pointerMove") {
      return {
        sessionId,
        selectedWindowId: "window-session-1",
        mappedEvent: {
          kind: "pointerMove" as const,
          position: {
            x: Math.floor((event.position.x / clientViewport.width) * 1280),
            y: Math.floor((event.position.y / clientViewport.height) * 720)
          }
        },
        status: "delivered" as const
      };
    }

    return {
      sessionId,
      selectedWindowId: "window-session-1",
      mappedEvent: event,
      status: event.kind === "focus" ? ("focused" as const) : ("delivered" as const)
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
      encoding: configuredEncoding,
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
      encoding: {
        ...configuredEncoding,
        state: "drained" as const
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

  async startAudioStream(sessionId: string, options: AudioStreamStartOptions) {
    return {
      id: "audio-stream-1",
      sessionId,
      selectedWindowId: "window-session-1",
      source: {
        scope: "selectedApplication" as const,
        selectedWindowId: "window-session-1",
        applicationId: "terminal",
        title: "Terminal"
      },
      backend: {
        controlPlane: "controlPlane" as const,
        plannedCapture: "pipeWire" as const,
        plannedPlayback: "pipeWire" as const,
        plannedMicrophone: "pipeWire" as const,
        statuses: pipeWireAudioBackendStatuses,
        readiness: "controlPlaneOnly" as const,
        notes: [
          "current stream enforces control-plane state only; native capture, playback, client microphone capture, and server microphone injection are not implemented"
        ]
      },
      devices: {
        outputDeviceId: options.outputDeviceId,
        inputDeviceId: options.inputDeviceId
      },
      microphone: options.microphone,
      mute: {
        systemAudioMuted: options.systemAudioMuted,
        microphoneMuted: options.microphoneMuted
      },
      capabilities: {
        systemAudio: { supported: true },
        microphoneCapture: { supported: true },
        microphoneInjection: {
          supported: false,
          reason: "server-side microphone injection backend is not implemented yet"
        },
        echoCancellation: { supported: true },
        deviceSelection: { supported: true }
      },
      stats: {
        packetsSent: 0,
        packetsReceived: 0,
        latencyMs: 0
      },
      health: {
        healthy: true,
        message: "audio stream started"
      },
      state: "streaming" as const
    };
  }

  async stopAudioStream(streamId: string) {
    return {
      ...(await this.audioStreamStatus(streamId)),
      health: {
        healthy: false,
        message: "audio stream stopped by client"
      },
      state: "stopped" as const
    };
  }

  async updateAudioStream(streamId: string, update: AudioStreamUpdate) {
    return {
      ...(await this.audioStreamStatus(streamId)),
      devices: {
        outputDeviceId: update.outputDeviceId,
        inputDeviceId: update.inputDeviceId
      },
      mute: {
        systemAudioMuted: update.systemAudioMuted,
        microphoneMuted: update.microphoneMuted
      },
      health: {
        healthy: true,
        message: "audio stream controls updated"
      }
    };
  }

  async audioStreamStatus(streamId: string) {
    return {
      id: streamId,
      sessionId: "session-1",
      selectedWindowId: "window-session-1",
      source: {
        scope: "selectedApplication" as const,
        selectedWindowId: "window-session-1",
        applicationId: "terminal",
        title: "Terminal"
      },
      backend: {
        controlPlane: "controlPlane" as const,
        plannedCapture: "pipeWire" as const,
        plannedPlayback: "pipeWire" as const,
        plannedMicrophone: "pipeWire" as const,
        statuses: pipeWireAudioBackendStatuses,
        readiness: "controlPlaneOnly" as const,
        notes: [
          "current stream enforces control-plane state only; native capture, playback, client microphone capture, and server microphone injection are not implemented"
        ]
      },
      devices: {},
      microphone: "disabled" as const,
      mute: {
        systemAudioMuted: false,
        microphoneMuted: true
      },
      capabilities: {
        systemAudio: { supported: true },
        microphoneCapture: { supported: true },
        microphoneInjection: {
          supported: false,
          reason: "server-side microphone injection backend is not implemented yet"
        },
        echoCancellation: { supported: true },
        deviceSelection: { supported: true }
      },
      stats: {
        packetsSent: 0,
        packetsReceived: 0,
        latencyMs: 0
      },
      health: {
        healthy: true,
        message: "audio stream started"
      },
      state: "streaming" as const
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
      encoding: configuredEncoding,
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
      encoding: {
        ...configuredEncoding,
        state: "encoding" as const,
        output: {
          framesSubmitted: 1,
          framesEncoded: 1,
          keyframesEncoded: 1,
          bytesProduced: 23032,
          lastFrame: {
            sequence: 1,
            timestampMs: 0,
            byteLength: 23032,
            keyframe: true
          }
        }
      },
      signaling: {
        kind: "webRtcOffer" as const,
        negotiationState: "negotiated" as const,
        offer: streamOffer,
        answer: clientAnswer,
        iceCandidates: [serverIceCandidate, ...clientIceCandidates]
      },
      stats: {
        framesEncoded: 1,
        bitrateKbps: 2764,
        latencyMs: 33,
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
      encoding: configuredEncoding,
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

  it("forwards input events through the service contract", async () => {
    const service = new FakeRemoteService();

    await expect(
      service.forwardInput("session-1", { width: 640, height: 360 }, { kind: "focus" })
    ).resolves.toMatchObject({
      sessionId: "session-1",
      selectedWindowId: "window-session-1",
      mappedEvent: {
        kind: "focus"
      },
      status: "focused"
    });
    await expect(
      service.forwardInput("session-1", { width: 640, height: 360 }, {
        kind: "pointerMove",
        position: { x: 320, y: 180 }
      })
    ).resolves.toMatchObject({
      mappedEvent: {
        kind: "pointerMove",
        position: {
          x: 640,
          y: 360
        }
      },
      status: "delivered"
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
      encoding: {
        contract: {
          codec: "h264",
          target: {
            resolution: {
              width: 1280,
              height: 720
            }
          },
          adaptation: {
            currentTarget: {
              width: 1280,
              height: 720
            },
            reason: "matchesViewport"
          }
        },
        state: "configured",
        output: {
          framesEncoded: 0,
          lastFrame: null
        }
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
      encoding: {
        state: "encoding",
        output: {
          framesSubmitted: 1,
          framesEncoded: 1,
          keyframesEncoded: 1,
          lastFrame: {
            sequence: 1,
            keyframe: true
          }
        }
      },
      stats: {
        framesEncoded: 1,
        bitrateKbps: 2764,
        latencyMs: 33
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

  it("starts, updates, checks, and stops audio streams", async () => {
    const service = new FakeRemoteService();

    await expect(
      service.startAudioStream("session-1", {
        microphone: "enabled",
        systemAudioMuted: false,
        microphoneMuted: true,
        outputDeviceId: "speakers",
        inputDeviceId: "mic"
      })
    ).resolves.toMatchObject({
      id: "audio-stream-1",
      sessionId: "session-1",
      microphone: "enabled",
      mute: {
        systemAudioMuted: false,
        microphoneMuted: true
      },
      capabilities: {
        systemAudio: {
          supported: true
        },
        microphoneCapture: {
          supported: true
        }
      },
      backend: {
        controlPlane: "controlPlane",
        plannedCapture: "pipeWire",
        plannedPlayback: "pipeWire",
        plannedMicrophone: "pipeWire",
        statuses: [
          expect.objectContaining({
            leg: "capture",
            backend: "pipeWire",
            available: false,
            failure: expect.objectContaining({
              kind: "nativeBackendNotImplemented"
            })
          }),
          expect.objectContaining({
            leg: "playback",
            backend: "pipeWire",
            available: false,
            failure: expect.objectContaining({
              kind: "nativeBackendNotImplemented"
            })
          }),
          expect.objectContaining({
            leg: "clientMicrophoneCapture",
            backend: "pipeWire",
            available: false,
            failure: expect.objectContaining({
              kind: "nativeBackendNotImplemented"
            })
          }),
          expect.objectContaining({
            leg: "serverMicrophoneInjection",
            backend: "pipeWire",
            available: false,
            failure: expect.objectContaining({
              kind: "nativeBackendNotImplemented"
            })
          })
        ],
        readiness: "controlPlaneOnly"
      },
      state: "streaming"
    });
    await expect(
      service.updateAudioStream("audio-stream-1", {
        systemAudioMuted: true,
        microphoneMuted: true,
        outputDeviceId: "headphones"
      })
    ).resolves.toMatchObject({
      devices: {
        outputDeviceId: "headphones"
      },
      mute: {
        systemAudioMuted: true
      },
      health: {
        message: "audio stream controls updated"
      }
    });
    await expect(service.audioStreamStatus("audio-stream-1")).resolves.toMatchObject({
      id: "audio-stream-1",
      state: "streaming"
    });
    await expect(service.stopAudioStream("audio-stream-1")).resolves.toMatchObject({
      id: "audio-stream-1",
      state: "stopped"
    });
  });
});
