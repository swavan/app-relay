import type { AppSummary, ApplicationSession, Capability, HealthStatus } from "./services";

export type ClientLoadState = "loading" | "empty" | "error" | "success";

export type AppViewModel = {
  loadState: ClientLoadState;
  connectionLabel: string;
  healthText: string;
  capabilitiesText: string;
  emptyText: string;
  errorText: string;
  sessionTitle: string;
  apps: AppSummary[];
};

export type AppViewModelInput = {
  health: HealthStatus | null;
  capabilities: Capability[];
  apps: AppSummary[];
  errorMessage: string;
  selectedProfileLabel: string | null;
  activeSession: ApplicationSession | null;
  loading: boolean;
};

export function buildAppViewModel(input: AppViewModelInput): AppViewModel {
  const supportedCapabilities = input.capabilities.filter((capability) => capability.supported);
  const loadState = resolveLoadState(input);

  return {
    loadState,
    connectionLabel: input.selectedProfileLabel ?? "Local development",
    healthText: input.health?.version ?? "checking",
    capabilitiesText: `${supportedCapabilities.length}/${input.capabilities.length}`,
    emptyText: "No applications found for this server.",
    errorText: input.errorMessage,
    sessionTitle: input.activeSession?.selectedWindow.title ?? "",
    apps: input.apps,
  };
}

function resolveLoadState(input: AppViewModelInput): ClientLoadState {
  if (input.loading) {
    return "loading";
  }

  if (input.errorMessage.trim() !== "") {
    return "error";
  }

  if (input.apps.length === 0) {
    return "empty";
  }

  return "success";
}
