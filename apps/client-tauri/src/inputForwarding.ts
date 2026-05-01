import type { ApplicationSession, Capability, InputDelivery, ViewportSize } from "./services";

export function inputViewportForSession(session: ApplicationSession): ViewportSize {
  return session.viewport;
}

export function inputModeFromDelivery(
  currentMode: boolean,
  delivery: InputDelivery
): boolean {
  switch (delivery.status) {
    case "focused":
      return true;
    case "blurred":
    case "ignoredBlurred":
      return false;
    case "delivered":
      return currentMode;
  }
}

export function centerPoint(viewport: ViewportSize) {
  return {
    x: Math.floor(viewport.width / 2),
    y: Math.floor(viewport.height / 2)
  };
}

export type InputControlAvailability = {
  focusAvailable: boolean;
  testTextAvailable: boolean;
  testClickAvailable: boolean;
};

export function inputControlAvailability(
  hasActiveSession: boolean,
  capabilities: Capability[]
): InputControlAvailability {
  return {
    focusAvailable: hasActiveSession,
    testTextAvailable: hasActiveSession && supportsFeature(capabilities, "keyboardInput"),
    testClickAvailable: hasActiveSession && supportsFeature(capabilities, "mouseInput")
  };
}

export function supportsFeature(capabilities: Capability[], feature: string): boolean {
  const expectedFeature = normalizeFeature(feature);
  return capabilities.some(
    (capability) => capability.supported && normalizeFeature(capability.feature) === expectedFeature
  );
}

function normalizeFeature(feature: string) {
  return feature.replace(/[-_\s]/g, "").toLowerCase();
}
