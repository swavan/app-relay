import type { ViewportSize } from "./services";

export type VideoStreamSession = {
  id: string;
  sessionId: string;
  selectedWindowId: string;
  viewport: ViewportSize;
  signaling: {
    kind: "webRtcOffer";
    offer?: string;
  };
  stats: {
    framesEncoded: number;
    bitrateKbps: number;
    latencyMs: number;
  };
  state: "starting" | "streaming" | "stopped" | "failed";
  failureReason?: string;
};
