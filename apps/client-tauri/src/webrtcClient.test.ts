import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DefaultPeerConnectionFactory,
  WebRtcClient,
  type PeerConnectionFactory,
  type PeerConnectionLike
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

type SignalingOnly = Pick<
  RemoteService,
  "submitSdpOffer" | "submitIceCandidate" | "signalEndOfCandidates" | "pollSignaling"
>;

interface FakeService extends SignalingOnly {
  readonly offers: Array<{ sessionId: string; sdp: string; role: SdpRole }>;
  readonly localCandidates: Array<{
    sessionId: string;
    direction: SignalingDirection;
    candidate: IceCandidateInput;
  }>;
  readonly endOfCandidates: Array<{ sessionId: string; direction: SignalingDirection }>;
  readonly polls: Array<{
    sessionId: string;
    direction: SignalingDirection;
    sinceSequence: number;
  }>;
  /** Queue messages to be returned by the next pollSignaling call. */
  enqueueAnswerer(
    messages: SignalingEnvelope[]
  ): void;
}

function buildFakeService(): FakeService {
  const offers: FakeService["offers"] = [];
  const localCandidates: FakeService["localCandidates"] = [];
  const endOfCandidates: FakeService["endOfCandidates"] = [];
  const polls: FakeService["polls"] = [];
  let sequence = 0;
  const queued: SignalingMessage[] = [];

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

  return {
    offers,
    localCandidates,
    endOfCandidates,
    polls,
    enqueueAnswerer(messages) {
      for (const env of messages) {
        sequence += 1;
        queued.push({ sequence, direction: "answererToOfferer", envelope: env });
      }
    },
    async submitSdpOffer(sessionId, sdp, role) {
      offers.push({ sessionId, sdp, role });
      return ack(sessionId, "offerToAnswerer", { kind: "sdpOffer", sdp, role });
    },
    async submitIceCandidate(sessionId, direction, candidate) {
      localCandidates.push({ sessionId, direction, candidate });
      return ack(sessionId, direction, {
        kind: "iceCandidate",
        candidate: candidate.candidate,
        sdpMid: candidate.sdpMid,
        sdpMlineIndex: candidate.sdpMlineIndex
      });
    },
    async signalEndOfCandidates(sessionId, direction) {
      endOfCandidates.push({ sessionId, direction });
      return ack(sessionId, direction, { kind: "endOfCandidates" });
    },
    async pollSignaling(sessionId, direction, sinceSequence): Promise<SignalingPoll> {
      polls.push({ sessionId, direction, sinceSequence });
      const messages = queued
        .filter((m) => m.direction === direction && m.sequence > sinceSequence)
        .slice();
      const lastSequence =
        messages.length > 0 ? messages[messages.length - 1].sequence : sinceSequence;
      return { sessionId, direction, lastSequence, messages };
    }
  };
}

interface FakePeerConnection extends PeerConnectionLike {
  readonly createdOffers: Array<{ type: "offer"; sdp: string }>;
  readonly localDescriptions: Array<{ type: "offer" | "answer"; sdp: string }>;
  readonly remoteDescriptions: Array<{ type: "offer" | "answer"; sdp: string }>;
  readonly addedIceCandidates: Array<{
    candidate: string;
    sdpMid: string;
    sdpMLineIndex: number;
  }>;
  readonly transceivers: Array<{ kind: "video" | "audio"; direction?: string }>;
  closed: boolean;
  fireIceCandidate(candidate: PeerConnectionLike["onicecandidate"] extends infer F
    ? F extends (event: { candidate: infer C }) => void
      ? C
      : never
    : never): void;
  fireTrack(stream: MediaStream): void;
}

function buildFakePeer(): FakePeerConnection {
  const createdOffers: FakePeerConnection["createdOffers"] = [];
  const localDescriptions: FakePeerConnection["localDescriptions"] = [];
  const remoteDescriptions: FakePeerConnection["remoteDescriptions"] = [];
  const addedIceCandidates: FakePeerConnection["addedIceCandidates"] = [];
  const transceivers: FakePeerConnection["transceivers"] = [];
  let offerCounter = 0;

  const peer: FakePeerConnection = {
    createdOffers,
    localDescriptions,
    remoteDescriptions,
    addedIceCandidates,
    transceivers,
    closed: false,
    onicecandidate: null,
    ontrack: null,
    oniceconnectionstatechange: null,
    iceConnectionState: "new",
    addTransceiver(kind, init) {
      transceivers.push({ kind, direction: init?.direction });
    },
    async createOffer() {
      offerCounter += 1;
      const offer = { type: "offer" as const, sdp: `v=0\r\no=- ${offerCounter} 1 IN IP4 0.0.0.0\r\n` };
      createdOffers.push(offer);
      return offer;
    },
    async setLocalDescription(description) {
      localDescriptions.push(description);
    },
    async setRemoteDescription(description) {
      remoteDescriptions.push(description);
    },
    async addIceCandidate(candidate) {
      addedIceCandidates.push(candidate);
    },
    close() {
      peer.closed = true;
    },
    fireIceCandidate(candidate) {
      peer.onicecandidate?.({ candidate });
    },
    fireTrack(stream) {
      peer.ontrack?.({ streams: [stream] });
    }
  };

  return peer;
}

function buildFactory(peer: FakePeerConnection): PeerConnectionFactory {
  return {
    create: () => peer
  };
}

function fakeStream(): MediaStream {
  return {} as unknown as MediaStream;
}

describe("WebRtcClient", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("creates an offer, submits it, and resolves with the MediaStream from ontrack", async () => {
    const services = buildFakeService();
    const peer = buildFakePeer();
    const client = new WebRtcClient({
      services,
      peerConnectionFactory: buildFactory(peer),
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000
    });

    const stream = fakeStream();
    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\nanswer\r\n" }]);

    const connectPromise = client.connect("session-1");
    // Let the negotiation microtasks run (createOffer, setLocalDescription, submitSdpOffer)
    // and the first poll tick.
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(20);

    expect(services.offers).toHaveLength(1);
    expect(services.offers[0]).toMatchObject({
      sessionId: "session-1",
      role: "offerer"
    });
    expect(services.offers[0].sdp).toBe(peer.createdOffers[0].sdp);
    expect(peer.transceivers).toEqual([{ kind: "video", direction: "recvonly" }]);

    peer.fireTrack(stream);
    await vi.advanceTimersByTimeAsync(20);

    await expect(connectPromise).resolves.toBe(stream);
    client.disconnect();
  });

  it("feeds remote ICE candidates from poll into the peer connection", async () => {
    const services = buildFakeService();
    const peer = buildFakePeer();
    const client = new WebRtcClient({
      services,
      peerConnectionFactory: buildFactory(peer),
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000
    });

    services.enqueueAnswerer([
      { kind: "sdpAnswer", sdp: "v=0\r\nanswer\r\n" },
      {
        kind: "iceCandidate",
        candidate: "candidate:remote 1 udp 100 192.0.2.1 9 typ host",
        sdpMid: "video",
        sdpMlineIndex: 0
      }
    ]);

    const connectPromise = client.connect("session-1");
    await vi.advanceTimersByTimeAsync(20);
    peer.fireTrack(fakeStream());
    await vi.advanceTimersByTimeAsync(20);
    await connectPromise;

    expect(peer.addedIceCandidates).toEqual([
      {
        candidate: "candidate:remote 1 udp 100 192.0.2.1 9 typ host",
        sdpMid: "video",
        sdpMLineIndex: 0
      }
    ]);
    client.disconnect();
  });

  it("submits local ICE candidates with valid sdpMid/sdpMLineIndex", async () => {
    const services = buildFakeService();
    const peer = buildFakePeer();
    const client = new WebRtcClient({
      services,
      peerConnectionFactory: buildFactory(peer),
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000
    });

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\n" }]);
    const connectPromise = client.connect("session-1");
    await vi.advanceTimersByTimeAsync(20);

    peer.fireIceCandidate({
      candidate: "candidate:local 1 udp 100 192.0.2.2 9 typ host",
      sdpMid: "video",
      sdpMLineIndex: 0
    });
    await vi.advanceTimersByTimeAsync(0);

    expect(services.localCandidates).toEqual([
      {
        sessionId: "session-1",
        direction: "offerToAnswerer",
        candidate: {
          candidate: "candidate:local 1 udp 100 192.0.2.2 9 typ host",
          sdpMid: "video",
          sdpMlineIndex: 0
        }
      }
    ]);
    peer.fireTrack(fakeStream());
    await vi.advanceTimersByTimeAsync(20);
    await connectPromise;
    client.disconnect();
  });

  it("skips local ICE candidates with null sdpMid or null sdpMLineIndex", async () => {
    const services = buildFakeService();
    const peer = buildFakePeer();
    const client = new WebRtcClient({
      services,
      peerConnectionFactory: buildFactory(peer),
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000
    });

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\n" }]);
    const connectPromise = client.connect("session-1");
    await vi.advanceTimersByTimeAsync(20);

    peer.fireIceCandidate({
      candidate: "candidate:local 1 udp 100 192.0.2.2 9 typ host",
      sdpMid: null,
      sdpMLineIndex: 0
    });
    peer.fireIceCandidate({
      candidate: "candidate:local 2 udp 100 192.0.2.3 9 typ host",
      sdpMid: "video",
      sdpMLineIndex: null
    });
    await vi.advanceTimersByTimeAsync(0);

    expect(services.localCandidates).toEqual([]);
    peer.fireTrack(fakeStream());
    await vi.advanceTimersByTimeAsync(20);
    await connectPromise;
    client.disconnect();
  });

  it("signals end-of-candidates when local gathering completes", async () => {
    const services = buildFakeService();
    const peer = buildFakePeer();
    const client = new WebRtcClient({
      services,
      peerConnectionFactory: buildFactory(peer),
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000
    });

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\n" }]);
    const connectPromise = client.connect("session-1");
    await vi.advanceTimersByTimeAsync(20);

    peer.fireIceCandidate(null);
    await vi.advanceTimersByTimeAsync(0);

    expect(services.endOfCandidates).toEqual([
      { sessionId: "session-1", direction: "offerToAnswerer" }
    ]);
    peer.fireTrack(fakeStream());
    await vi.advanceTimersByTimeAsync(20);
    await connectPromise;
    client.disconnect();
  });

  it("rejects on poll timeout with no track", async () => {
    const services = buildFakeService();
    const peer = buildFakePeer();
    const stateChanges: string[] = [];
    const client = new WebRtcClient({
      services,
      peerConnectionFactory: buildFactory(peer),
      pollIntervalMs: 10,
      pollTimeoutMs: 50,
      onIceStateChange: (state) => stateChanges.push(state)
    });

    const connectPromise = client.connect("session-1");
    // Surface unhandled rejection if it slips through.
    const observed = connectPromise.catch((error: unknown) => error);

    await vi.advanceTimersByTimeAsync(60);
    const result = await observed;
    expect(result).toBeInstanceOf(Error);
    expect((result as Error).message).toMatch(/timed out/);
    expect(stateChanges).toContain("failed");
    client.disconnect();
  });

  it("disconnect aborts the poll loop and closes the peer connection", async () => {
    const services = buildFakeService();
    const peer = buildFakePeer();
    const client = new WebRtcClient({
      services,
      peerConnectionFactory: buildFactory(peer),
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000
    });

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\n" }]);
    const connectPromise = client.connect("session-1");
    const observed = connectPromise.catch((error: unknown) => error);
    await vi.advanceTimersByTimeAsync(20);
    peer.fireTrack(fakeStream());
    await vi.advanceTimersByTimeAsync(20);
    await observed;

    client.disconnect();
    expect(peer.closed).toBe(true);

    const pollsBefore = services.polls.length;
    const remoteCountBefore = peer.remoteDescriptions.length;
    const iceCountBefore = peer.addedIceCandidates.length;

    // Even if the server still has messages buffered, the aborted client
    // must not apply them.
    services.enqueueAnswerer([
      { kind: "sdpAnswer", sdp: "v=0\r\nshould-not-apply\r\n" },
      {
        kind: "iceCandidate",
        candidate: "candidate:late 1 udp 100 192.0.2.4 9 typ host",
        sdpMid: "video",
        sdpMlineIndex: 0
      }
    ]);
    await vi.advanceTimersByTimeAsync(200);

    expect(services.polls.length).toBe(pollsBefore);
    expect(peer.remoteDescriptions.length).toBe(remoteCountBefore);
    expect(peer.addedIceCandidates.length).toBe(iceCountBefore);

    // Idempotent.
    expect(() => client.disconnect()).not.toThrow();
  });

  it("updates sinceSequence after each poll", async () => {
    const services = buildFakeService();
    const peer = buildFakePeer();
    const client = new WebRtcClient({
      services,
      peerConnectionFactory: buildFactory(peer),
      pollIntervalMs: 10,
      pollTimeoutMs: 5_000
    });

    services.enqueueAnswerer([{ kind: "sdpAnswer", sdp: "v=0\r\nfirst\r\n" }]);
    const connectPromise = client.connect("session-1");
    await vi.advanceTimersByTimeAsync(20);

    expect(services.polls.length).toBeGreaterThanOrEqual(1);
    const firstPoll = services.polls[0];
    expect(firstPoll.sinceSequence).toBe(0);

    // Drive at least one more poll cycle.
    await vi.advanceTimersByTimeAsync(20);

    expect(services.polls.length).toBeGreaterThanOrEqual(2);
    const secondPoll = services.polls[1];
    expect(secondPoll.sinceSequence).toBeGreaterThanOrEqual(1);

    peer.fireTrack(fakeStream());
    await vi.advanceTimersByTimeAsync(20);
    await connectPromise;
    client.disconnect();
  });
});

describe("DefaultPeerConnectionFactory", () => {
  it("constructs an RTCPeerConnection-shaped object via the documented cast", () => {
    const RTCPeerConnectionStub = vi
      .fn()
      .mockImplementation(() => ({ iceConnectionState: "new" }) as unknown as RTCPeerConnection);
    const original = (globalThis as { RTCPeerConnection?: unknown }).RTCPeerConnection;
    (globalThis as { RTCPeerConnection?: unknown }).RTCPeerConnection =
      RTCPeerConnectionStub as unknown as typeof RTCPeerConnection;
    try {
      const factory = new DefaultPeerConnectionFactory();
      const pc = factory.create();
      expect(RTCPeerConnectionStub).toHaveBeenCalledWith({ iceServers: [] });
      expect(pc.iceConnectionState).toBe("new");
    } finally {
      (globalThis as { RTCPeerConnection?: unknown }).RTCPeerConnection = original;
    }
  });
});
