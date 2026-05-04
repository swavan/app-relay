import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  WebRtcVideoSession,
  type WebRtcVideoSessionState
} from "./webrtcVideoSession";
import type {
  PeerConnectionFactory,
  PeerConnectionLike
} from "./webrtcClient";
import type {
  IceCandidateInput,
  RemoteService,
  SdpRole,
  SignalingDirection,
  SignalingEnvelope,
  SignalingMessage,
  SignalingPoll,
  SignalingSubmitAck
} from "./services";
import type { VideoStreamSession } from "./videoStreams";

interface FakeRemoteService extends RemoteService {
  readonly polls: Array<{
    sessionId: string;
    direction: SignalingDirection;
    sinceSequence: number;
  }>;
  readonly offers: Array<{ sessionId: string; sdp: string; role: SdpRole }>;
  enqueueAnswerer(messages: SignalingEnvelope[]): void;
  failNextPoll(error: Error): void;
}

function buildFakeService(): FakeRemoteService {
  const offers: FakeRemoteService["offers"] = [];
  const polls: FakeRemoteService["polls"] = [];
  let sequence = 0;
  const queued: SignalingMessage[] = [];
  const pollErrors: Error[] = [];

  const ack = (
    sessionId: string,
    direction: SignalingDirection,
    envelope: SignalingEnvelope
  ): SignalingSubmitAck => {
    sequence += 1;
    return {
      sessionId,
      direction,
      sequence,
      envelopeKind: envelope.kind,
      payloadByteLength: 0
    };
  };

  const unimplemented = (label: string): never => {
    throw new Error(`FakeRemoteService.${label} not implemented in test`);
  };

  return {
    polls,
    offers,
    enqueueAnswerer(messages) {
      for (const env of messages) {
        sequence += 1;
        queued.push({ sequence, direction: "answererToOfferer", envelope: env });
      }
    },
    failNextPoll(error) {
      pollErrors.push(error);
    },
    async submitSdpOffer(sessionId, sdp, role) {
      offers.push({ sessionId, sdp, role });
      return ack(sessionId, "offerToAnswerer", { kind: "sdpOffer", sdp, role });
    },
    async submitIceCandidate(
      sessionId,
      direction,
      candidate: IceCandidateInput
    ) {
      return ack(sessionId, direction, {
        kind: "iceCandidate",
        candidate: candidate.candidate,
        sdpMid: candidate.sdpMid,
        sdpMlineIndex: candidate.sdpMlineIndex
      });
    },
    async signalEndOfCandidates(sessionId, direction) {
      return ack(sessionId, direction, { kind: "endOfCandidates" });
    },
    async pollSignaling(sessionId, direction, sinceSequence): Promise<SignalingPoll> {
      polls.push({ sessionId, direction, sinceSequence });
      const next = pollErrors.shift();
      if (next) {
        throw next;
      }
      const messages = queued
        .filter((m) => m.direction === direction && m.sequence > sinceSequence)
        .slice();
      const lastSequence =
        messages.length > 0 ? messages[messages.length - 1].sequence : sinceSequence;
      return { sessionId, direction, lastSequence, messages };
    },
    async health() {
      return unimplemented("health");
    },
    async capabilities() {
      return unimplemented("capabilities");
    },
    async applications() {
      return unimplemented("applications");
    },
    async activeSessions() {
      return unimplemented("activeSessions");
    },
    async createSession() {
      return unimplemented("createSession");
    },
    async resizeSession() {
      return unimplemented("resizeSession");
    },
    async closeSession() {
      return unimplemented("closeSession");
    },
    async forwardInput() {
      return unimplemented("forwardInput");
    },
    async activeInputFocus() {
      return unimplemented("activeInputFocus");
    },
    async activeVideoStreams() {
      return unimplemented("activeVideoStreams");
    },
    async startVideoStream() {
      return unimplemented("startVideoStream");
    },
    async stopVideoStream() {
      return unimplemented("stopVideoStream");
    },
    async activeAudioStreams() {
      return unimplemented("activeAudioStreams");
    },
    async startAudioStream() {
      return unimplemented("startAudioStream");
    },
    async stopAudioStream() {
      return unimplemented("stopAudioStream");
    },
    async updateAudioStream() {
      return unimplemented("updateAudioStream");
    },
    async audioStreamStatus() {
      return unimplemented("audioStreamStatus");
    },
    async reconnectVideoStream() {
      return unimplemented("reconnectVideoStream");
    },
    async negotiateVideoStream() {
      return unimplemented("negotiateVideoStream");
    },
    async videoStreamStatus() {
      return unimplemented("videoStreamStatus");
    },
    async submitSdpAnswer(sessionId, sdp) {
      return ack(sessionId, "answererToOfferer", { kind: "sdpAnswer", sdp });
    }
  };
}

interface FakePeerConnection extends PeerConnectionLike {
  closed: boolean;
  fireTrack(stream: MediaStream): void;
}

function buildFakePeer(): FakePeerConnection {
  let offerCounter = 0;
  const peer: FakePeerConnection = {
    closed: false,
    onicecandidate: null,
    ontrack: null,
    oniceconnectionstatechange: null,
    iceConnectionState: "new",
    addTransceiver() {},
    async createOffer() {
      offerCounter += 1;
      return { type: "offer" as const, sdp: `v=0\r\no=- ${offerCounter}\r\n` };
    },
    async setLocalDescription() {},
    async setRemoteDescription() {},
    async addIceCandidate() {},
    close() {
      peer.closed = true;
    },
    fireTrack(stream) {
      peer.ontrack?.({ streams: [stream] });
    }
  };
  return peer;
}

interface QueueingFactory extends PeerConnectionFactory {
  readonly peers: FakePeerConnection[];
}

function buildQueueingFactory(): QueueingFactory {
  const peers: FakePeerConnection[] = [];
  return {
    peers,
    create(): PeerConnectionLike {
      const peer = buildFakePeer();
      peers.push(peer);
      return peer;
    }
  };
}

function fakeStream(label = "stream"): MediaStream {
  return { _label: label } as unknown as MediaStream;
}

function buildStreamSession(id: string, sessionId: string): VideoStreamSession {
  return {
    id,
    sessionId,
    selectedWindowId: `window-${sessionId}`,
    viewport: { width: 1280, height: 720 },
    captureSource: {
      scope: "selectedWindow",
      selectedWindowId: `window-${sessionId}`,
      applicationId: "app",
      title: "Title"
    },
    encoding: {
      contract: {
        codec: "h264",
        pixelFormat: "rgba",
        hardwareAcceleration: "none",
        target: {
          resolution: { width: 1280, height: 720 },
          maxFps: 30,
          targetBitrateKbps: 2500,
          keyframeIntervalFrames: 60
        },
        adaptation: {
          requestedViewport: { width: 1280, height: 720 },
          currentTarget: { width: 1280, height: 720 },
          limits: { maxWidth: 1920, maxHeight: 1080, maxPixels: 1920 * 1080 },
          reason: "matchesViewport"
        }
      },
      state: "configured",
      output: {
        framesSubmitted: 0,
        framesEncoded: 0,
        keyframesEncoded: 0,
        bytesProduced: 0,
        lastFrame: null
      }
    },
    signaling: {
      kind: "webRtcOffer",
      negotiationState: "awaitingAnswer",
      iceCandidates: []
    },
    stats: {
      framesEncoded: 0,
      bitrateKbps: 0,
      latencyMs: 0,
      reconnectAttempts: 0
    },
    health: { healthy: true },
    state: "starting"
  };
}

describe("WebRtcVideoSession", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("attach starts a new client and transitions through connecting -> connected when ontrack fires", async () => {
    const services = buildFakeService();
    const factory = buildQueueingFactory();
    const states: WebRtcVideoSessionState[] = [];
    const session = new WebRtcVideoSession({
      services,
      peerConnectionFactory: factory,
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000,
      onStateChange: (s) => states.push(s)
    });

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\nanswer\r\n" }]);
    session.attach(buildStreamSession("stream-A", "session-A"));

    expect(states).toEqual([{ kind: "connecting", streamId: "stream-A" }]);

    await vi.advanceTimersByTimeAsync(20);
    expect(factory.peers).toHaveLength(1);
    factory.peers[0].fireTrack(fakeStream("media-A"));
    await vi.advanceTimersByTimeAsync(20);

    expect(states).toHaveLength(2);
    expect(states[1].kind).toBe("connected");
    if (states[1].kind === "connected") {
      expect(states[1].streamId).toBe("stream-A");
      expect(states[1].mediaStream).toEqual(fakeStream("media-A"));
    }
    session.detach();
  });

  it("attach with the same stream id is a no-op", async () => {
    const services = buildFakeService();
    const factory = buildQueueingFactory();
    const states: WebRtcVideoSessionState[] = [];
    const session = new WebRtcVideoSession({
      services,
      peerConnectionFactory: factory,
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000,
      onStateChange: (s) => states.push(s)
    });

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\n" }]);
    const stream = buildStreamSession("stream-A", "session-A");
    session.attach(stream);
    await vi.advanceTimersByTimeAsync(20);

    const offerCountBefore = services.offers.length;
    session.attach(stream);
    await vi.advanceTimersByTimeAsync(20);

    expect(factory.peers).toHaveLength(1);
    expect(services.offers.length).toBe(offerCountBefore);

    session.detach();
  });

  it("attach with a different stream id disconnects the previous client and starts a new one", async () => {
    const services = buildFakeService();
    const factory = buildQueueingFactory();
    const states: WebRtcVideoSessionState[] = [];
    const session = new WebRtcVideoSession({
      services,
      peerConnectionFactory: factory,
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000,
      onStateChange: (s) => states.push(s)
    });

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\n" }]);
    session.attach(buildStreamSession("stream-A", "session-A"));
    await vi.advanceTimersByTimeAsync(20);

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\nB\r\n" }]);
    session.attach(buildStreamSession("stream-B", "session-B"));
    await vi.advanceTimersByTimeAsync(20);

    expect(factory.peers).toHaveLength(2);
    expect(factory.peers[0].closed).toBe(true);
    expect(factory.peers[1].closed).toBe(false);
    expect(services.offers.map((o) => o.sessionId)).toEqual(["session-A", "session-B"]);

    expect(states.map((s) => s.kind)).toEqual(["connecting", "connecting"]);
    if (states[1].kind === "connecting") {
      expect(states[1].streamId).toBe("stream-B");
    }

    session.detach();
  });

  it("detach disconnects the current client and transitions to idle, idempotent", async () => {
    const services = buildFakeService();
    const factory = buildQueueingFactory();
    const states: WebRtcVideoSessionState[] = [];
    const session = new WebRtcVideoSession({
      services,
      peerConnectionFactory: factory,
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000,
      onStateChange: (s) => states.push(s)
    });

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\n" }]);
    session.attach(buildStreamSession("stream-A", "session-A"));
    await vi.advanceTimersByTimeAsync(20);

    session.detach();
    expect(factory.peers[0].closed).toBe(true);
    expect(states[states.length - 1]).toEqual({ kind: "idle" });

    const stateCount = states.length;
    session.detach();
    expect(states.length).toBe(stateCount);
  });

  it("detach before the promise resolves does NOT transition to connected after the late MediaStream arrives", async () => {
    const services = buildFakeService();
    const factory = buildQueueingFactory();
    const states: WebRtcVideoSessionState[] = [];
    const session = new WebRtcVideoSession({
      services,
      peerConnectionFactory: factory,
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000,
      onStateChange: (s) => states.push(s)
    });

    // Don't queue any sdpAnswer — but we'll fire ontrack manually after detach
    // to simulate a late stream resolution that should be discarded.
    session.attach(buildStreamSession("stream-A", "session-A"));
    await vi.advanceTimersByTimeAsync(20);

    session.detach();
    // The peer is closed by disconnect, but ontrack handler is still wired up
    // on the WebRtcClient's promise. Firing ontrack after detach must not
    // produce a "connected" state because the attempt has been replaced.
    factory.peers[0].fireTrack(fakeStream("late"));
    await vi.advanceTimersByTimeAsync(20);

    expect(states.map((s) => s.kind)).toEqual(["connecting", "idle"]);
  });

  it("connect rejection transitions to failed with the error message", async () => {
    const services = buildFakeService();
    const factory = buildQueueingFactory();
    const states: WebRtcVideoSessionState[] = [];
    const session = new WebRtcVideoSession({
      services,
      peerConnectionFactory: factory,
      pollIntervalMs: 10,
      pollTimeoutMs: 30,
      onStateChange: (s) => states.push(s)
    });

    session.attach(buildStreamSession("stream-A", "session-A"));
    // No sdpAnswer queued, no track fired — let the timeout fire.
    await vi.advanceTimersByTimeAsync(60);

    expect(states).toHaveLength(2);
    expect(states[1].kind).toBe("failed");
    if (states[1].kind === "failed") {
      expect(states[1].streamId).toBe("stream-A");
      expect(states[1].error).toMatch(/timed out/);
    }
    session.detach();
  });
});
