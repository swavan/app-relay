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
  apps: AppListItem[];
};

export type AppListItem = AppSummary & {
  iconView: AppIconView;
};

export type AppIconView =
  | {
      kind: "image";
      url: string;
      label: string;
      title: string;
    }
  | {
      kind: "label";
      label: string;
      title: string;
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
    apps: input.apps.map(buildAppListItem),
  };
}

function buildAppListItem(app: AppSummary): AppListItem {
  return {
    ...app,
    iconView: buildAppIconView(app),
  };
}

function buildAppIconView(app: AppSummary): AppIconView {
  const title = app.icon?.source ?? app.name;

  if (app.icon?.dataUrl) {
    return {
      kind: "image",
      url: app.icon.dataUrl,
      label: app.name,
      title,
    };
  }

  return {
    kind: "label",
    label: appInitials(app.name),
    title,
  };
}

function appInitials(name: string): string {
  const words = name
    .trim()
    .split(/\s+/)
    .filter((word) => word.length > 0);

  if (words.length === 0) {
    return "?";
  }

  if (words.length === 1) {
    return words[0].slice(0, 2).toUpperCase();
  }

  return `${words[0][0]}${words[1][0]}`.toUpperCase();
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
