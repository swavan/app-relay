import type { ApplicationSession, InputDelivery, ViewportSize } from "./services";

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
