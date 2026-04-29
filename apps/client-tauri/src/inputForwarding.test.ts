import { describe, expect, it } from "vitest";
import {
  centerPoint,
  inputModeFromDelivery,
  inputViewportForSession
} from "./inputForwarding";
import type { ApplicationSession, InputDelivery } from "./services";

const session: ApplicationSession = {
  id: "session-1",
  applicationId: "terminal",
  selectedWindow: {
    id: "window-session-1",
    applicationId: "terminal",
    title: "Terminal",
    selectionMethod: "launchIntent"
  },
  viewport: {
    width: 1280,
    height: 720
  },
  state: "ready"
};

function delivery(status: InputDelivery["status"]): InputDelivery {
  return {
    sessionId: "session-1",
    selectedWindowId: "window-session-1",
    mappedEvent: {
      kind: "focus"
    },
    status
  };
}

describe("input forwarding helpers", () => {
  it("uses the active session viewport for input forwarding", () => {
    expect(inputViewportForSession(session)).toEqual({ width: 1280, height: 720 });
    expect(centerPoint(inputViewportForSession(session))).toEqual({ x: 640, y: 360 });
  });

  it("derives input mode from focus and blur delivery status", () => {
    expect(inputModeFromDelivery(false, delivery("focused"))).toBe(true);
    expect(inputModeFromDelivery(true, delivery("blurred"))).toBe(false);
    expect(inputModeFromDelivery(true, delivery("ignoredBlurred"))).toBe(false);
    expect(inputModeFromDelivery(true, delivery("delivered"))).toBe(true);
  });
});
