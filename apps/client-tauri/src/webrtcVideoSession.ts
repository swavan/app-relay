import type { RemoteService } from "./services";
import type { VideoStreamSession } from "./videoStreams";
import { WebRtcClient, type PeerConnectionFactory } from "./webrtcClient";

export type WebRtcVideoSessionState =
  | { kind: "idle" }
  | { kind: "connecting"; streamId: string }
  | { kind: "connected"; streamId: string; mediaStream: MediaStream }
  | { kind: "failed"; streamId: string; error: string };

export interface WebRtcVideoSessionOptions {
  services: RemoteService;
  peerConnectionFactory?: PeerConnectionFactory;
  pollIntervalMs?: number;
  pollTimeoutMs?: number;
  /** Notified on every state transition. */
  onStateChange: (state: WebRtcVideoSessionState) => void;
}

interface ActiveAttempt {
  readonly attemptId: number;
  readonly streamId: string;
  readonly client: WebRtcClient;
}

export class WebRtcVideoSession {
  private readonly services: RemoteService;
  private readonly peerConnectionFactory?: PeerConnectionFactory;
  private readonly pollIntervalMs?: number;
  private readonly pollTimeoutMs?: number;
  private readonly onStateChange: (state: WebRtcVideoSessionState) => void;

  private active: ActiveAttempt | null = null;
  private nextAttemptId = 1;

  constructor(options: WebRtcVideoSessionOptions) {
    this.services = options.services;
    this.peerConnectionFactory = options.peerConnectionFactory;
    this.pollIntervalMs = options.pollIntervalMs;
    this.pollTimeoutMs = options.pollTimeoutMs;
    this.onStateChange = options.onStateChange;
  }

  /**
   * Begin (or restart) negotiation for the given stream session. If a
   * negotiation is already running for the same stream id, this is a
   * no-op. If a different stream is active, the previous client is
   * disconnected first.
   */
  attach(stream: VideoStreamSession): void {
    if (this.active !== null && this.active.streamId === stream.id) {
      return;
    }
    if (this.active !== null) {
      this.active.client.disconnect();
      this.active = null;
    }

    const attemptId = this.nextAttemptId++;
    const client = new WebRtcClient({
      services: this.services,
      peerConnectionFactory: this.peerConnectionFactory,
      pollIntervalMs: this.pollIntervalMs,
      pollTimeoutMs: this.pollTimeoutMs
    });
    this.active = { attemptId, streamId: stream.id, client };

    this.onStateChange({ kind: "connecting", streamId: stream.id });

    client
      .connect(stream.sessionId)
      .then((mediaStream) => {
        if (this.active === null || this.active.attemptId !== attemptId) {
          return;
        }
        this.onStateChange({
          kind: "connected",
          streamId: stream.id,
          mediaStream
        });
      })
      .catch((error: unknown) => {
        if (this.active === null || this.active.attemptId !== attemptId) {
          return;
        }
        const message = error instanceof Error ? error.message : String(error);
        this.onStateChange({ kind: "failed", streamId: stream.id, error: message });
      });
  }

  /**
   * Tear down the in-flight WebRTC client (if any). Idempotent.
   * Transitions state back to `idle`.
   */
  detach(): void {
    if (this.active === null) {
      return;
    }
    this.active.client.disconnect();
    this.active = null;
    this.onStateChange({ kind: "idle" });
  }
}
