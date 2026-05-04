import type { IceCandidateInput, RemoteService, SignalingMessage } from "./services";

/**
 * Subset of `RTCPeerConnection` we actually rely on. The native browser type
 * is structurally compatible at the call sites we use, so production code
 * casts `new RTCPeerConnection(...)` through `as unknown as PeerConnectionLike`.
 */
export interface PeerConnectionLike {
  addTransceiver(
    kind: "video" | "audio",
    init?: { direction?: "recvonly" | "sendrecv" | "sendonly" | "inactive" }
  ): void;
  createOffer(): Promise<{ type: "offer"; sdp: string }>;
  setLocalDescription(description: { type: "offer" | "answer"; sdp: string }): Promise<void>;
  setRemoteDescription(description: { type: "offer" | "answer"; sdp: string }): Promise<void>;
  addIceCandidate(candidate: {
    candidate: string;
    sdpMid: string;
    sdpMLineIndex: number;
  }): Promise<void>;
  close(): void;

  onicecandidate:
    | ((event: {
        candidate: { candidate: string; sdpMid: string | null; sdpMLineIndex: number | null } | null;
      }) => void)
    | null;
  ontrack: ((event: { streams: ReadonlyArray<MediaStream> }) => void) | null;
  oniceconnectionstatechange: (() => void) | null;
  iceConnectionState:
    | "new"
    | "checking"
    | "connected"
    | "completed"
    | "failed"
    | "disconnected"
    | "closed";
}

export interface PeerConnectionFactory {
  create(): PeerConnectionLike;
}

export class DefaultPeerConnectionFactory implements PeerConnectionFactory {
  create(): PeerConnectionLike {
    // The native `RTCPeerConnection` API is structurally compatible with
    // `PeerConnectionLike` for the methods/events we use, but TypeScript's
    // `RTCPeerConnection` exposes a much wider surface. The single cast
    // through `unknown` is the documented bridge for the browser runtime.
    const pc = new RTCPeerConnection({ iceServers: [] });
    return pc as unknown as PeerConnectionLike;
  }
}

export interface WebRtcClientOptions {
  services: Pick<
    RemoteService,
    "submitSdpOffer" | "submitIceCandidate" | "signalEndOfCandidates" | "pollSignaling"
  >;
  peerConnectionFactory?: PeerConnectionFactory;
  pollIntervalMs?: number;
  pollTimeoutMs?: number;
  onIceStateChange?: (state: string) => void;
}

const DEFAULT_POLL_INTERVAL_MS = 250;
const DEFAULT_POLL_TIMEOUT_MS = 30_000;

export class WebRtcClient {
  private readonly services: WebRtcClientOptions["services"];
  private readonly factory: PeerConnectionFactory;
  private readonly pollIntervalMs: number;
  private readonly pollTimeoutMs: number;
  private readonly onIceStateChange?: (state: string) => void;

  private pc: PeerConnectionLike | null = null;
  private aborted = false;
  private remoteAnswerApplied = false;
  private remoteEndOfCandidates = false;
  private sinceSequence = 0;

  constructor(options: WebRtcClientOptions) {
    this.services = options.services;
    this.factory = options.peerConnectionFactory ?? new DefaultPeerConnectionFactory();
    this.pollIntervalMs = options.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS;
    this.pollTimeoutMs = options.pollTimeoutMs ?? DEFAULT_POLL_TIMEOUT_MS;
    this.onIceStateChange = options.onIceStateChange;
  }

  async connect(sessionId: string): Promise<MediaStream> {
    if (this.pc !== null) {
      throw new Error("WebRtcClient.connect called while an existing connection is in progress");
    }
    this.aborted = false;
    this.remoteAnswerApplied = false;
    this.remoteEndOfCandidates = false;
    this.sinceSequence = 0;

    const pc = this.factory.create();
    this.pc = pc;

    return new Promise<MediaStream>((resolve, reject) => {
      let settled = false;
      const fail = (error: Error) => {
        if (settled) return;
        settled = true;
        this.onIceStateChange?.("failed");
        reject(error);
      };
      const succeed = (stream: MediaStream) => {
        if (settled) return;
        settled = true;
        resolve(stream);
      };

      pc.ontrack = (event) => {
        const stream = event.streams[0];
        if (stream) succeed(stream);
      };
      pc.oniceconnectionstatechange = () => {
        this.onIceStateChange?.(pc.iceConnectionState);
      };
      pc.onicecandidate = (event) => {
        if (this.aborted) return;
        const candidate = event.candidate;
        if (candidate === null) {
          this.services.signalEndOfCandidates(sessionId, "offerToAnswerer").catch((error) => {
            console.warn("WebRtcClient: signalEndOfCandidates failed", error);
          });
          return;
        }
        if (candidate.sdpMid === null || candidate.sdpMLineIndex === null) {
          // Server-side signaling validator requires both sdpMid and
          // sdpMLineIndex; skip silently when either is missing.
          return;
        }
        const input: IceCandidateInput = {
          candidate: candidate.candidate,
          sdpMid: candidate.sdpMid,
          sdpMlineIndex: candidate.sdpMLineIndex
        };
        this.services
          .submitIceCandidate(sessionId, "offerToAnswerer", input)
          .catch((error) => {
            console.warn("WebRtcClient: submitIceCandidate failed", error);
          });
      };

      const negotiate = async () => {
        pc.addTransceiver("video", { direction: "recvonly" });
        const offer = await pc.createOffer();
        await pc.setLocalDescription(offer);
        await this.services.submitSdpOffer(sessionId, offer.sdp, "offerer");
      };

      negotiate().catch((error) => {
        fail(error instanceof Error ? error : new Error(String(error)));
      });

      const timeoutHandle = setTimeout(() => {
        fail(new Error(`WebRtcClient.connect timed out after ${this.pollTimeoutMs}ms`));
      }, this.pollTimeoutMs);

      const pollLoop = async () => {
        while (!this.aborted && !settled) {
          try {
            const poll = await this.services.pollSignaling(
              sessionId,
              "answererToOfferer",
              this.sinceSequence
            );
            if (this.aborted || settled) break;
            await this.applyMessages(pc, poll.messages, fail);
            const seqs = poll.messages.map((m) => m.sequence);
            this.sinceSequence = Math.max(this.sinceSequence, poll.lastSequence, ...seqs);
          } catch (error) {
            fail(error instanceof Error ? error : new Error(String(error)));
            break;
          }
          if (this.aborted || settled) break;
          await delay(this.pollIntervalMs);
        }
      };

      void pollLoop().finally(() => {
        clearTimeout(timeoutHandle);
      });
    });
  }

  disconnect(): void {
    this.aborted = true;
    if (this.pc !== null) {
      try {
        this.pc.close();
      } catch (error) {
        console.warn("WebRtcClient: peer connection close failed", error);
      }
      this.pc = null;
    }
  }

  private async applyMessages(
    pc: PeerConnectionLike,
    messages: ReadonlyArray<SignalingMessage>,
    fail: (error: Error) => void
  ): Promise<void> {
    for (const message of messages) {
      if (this.aborted) return;
      const env = message.envelope;
      if (env.kind === "sdpAnswer") {
        if (this.remoteAnswerApplied) continue;
        try {
          await pc.setRemoteDescription({ type: "answer", sdp: env.sdp });
          this.remoteAnswerApplied = true;
        } catch (error) {
          fail(error instanceof Error ? error : new Error(String(error)));
          return;
        }
      } else if (env.kind === "iceCandidate") {
        if (this.remoteEndOfCandidates) continue;
        try {
          await pc.addIceCandidate({
            candidate: env.candidate,
            sdpMid: env.sdpMid,
            sdpMLineIndex: env.sdpMlineIndex
          });
        } catch (error) {
          console.warn("WebRtcClient: addIceCandidate failed", error);
        }
      } else if (env.kind === "endOfCandidates") {
        this.remoteEndOfCandidates = true;
      }
      // sdpOffer envelopes are not expected on the answerer→offerer channel; ignore.
    }
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
