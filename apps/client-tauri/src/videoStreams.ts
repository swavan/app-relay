import type { ViewportSize } from "./services";

export type VideoStreamSession = {
  id: string;
  sessionId: string;
  selectedWindowId: string;
  viewport: ViewportSize;
  captureSource: {
    scope: "selectedWindow";
    selectedWindowId: string;
    applicationId: string;
    title: string;
  };
  signaling: {
    kind: "webRtcOffer";
    offer?: string;
  };
  stats: {
    framesEncoded: number;
    bitrateKbps: number;
    latencyMs: number;
    reconnectAttempts: number;
  };
  health: {
    healthy: boolean;
    message?: string;
  };
  state: "starting" | "streaming" | "stopped" | "failed";
  failureReason?: string;
};
