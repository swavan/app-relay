import { describe, expect, it } from "vitest";
import {
  centerPoint,
  inputControlAvailability,
  inputModeFromDelivery,
  inputViewportForSession,
  supportsFeature
} from "./inputForwarding";
import type { ApplicationSession, Capability, InputDelivery } from "./services";

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

const capabilities: Capability[] = [
  {
    platform: "macos",
    feature: "keyboard-input",
    supported: true,
    reason: "System Events"
  },
  {
    platform: "macos",
    feature: "mouseInput",
    supported: false,
    reason: "planned"
  }
];

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

  it("normalizes server feature keys when checking input capabilities", () => {
    expect(supportsFeature(capabilities, "keyboardInput")).toBe(true);
    expect(supportsFeature(capabilities, "keyboard-input")).toBe(true);
    expect(supportsFeature(capabilities, "mouseInput")).toBe(false);
  });

  it("keeps focus available for active sessions and gates test controls by capability", () => {
    expect(inputControlAvailability(true, capabilities)).toEqual({
      focusAvailable: true,
      testTextAvailable: true,
      testClickAvailable: false
    });
    expect(
      inputControlAvailability(true, [
        ...capabilities,
        {
          platform: "linux",
          feature: "mouseInput",
          supported: true
        }
      ])
    ).toEqual({
      focusAvailable: true,
      testTextAvailable: true,
      testClickAvailable: true
    });
    expect(inputControlAvailability(false, capabilities)).toEqual({
      focusAvailable: false,
      testTextAvailable: false,
      testClickAvailable: false
    });
  });
});
