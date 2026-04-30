import type { AppSummary, ApplicationSession, Capability, HealthStatus } from "./services";

export type ClientLoadState = "loading" | "empty" | "error" | "success";

export type AppViewModel = {
  loadState: ClientLoadState;
  connectionLabel: string;
  healthText: string;
  capabilitiesText: string;
  unsupportedCapabilities: UnsupportedCapabilityView[];
  emptyText: string;
  errorText: string;
  sessionTitle: string;
  apps: AppListItem[];
};

export type UnsupportedCapabilityView = {
  feature: string;
  platform: string;
  reason: string;
};

export type AppListItem = AppSummary & {
  iconView: AppIconView;
  launchLabel: string;
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
    unsupportedCapabilities: input.capabilities
      .filter((capability) => !capability.supported)
      .map(buildUnsupportedCapabilityView),
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
    launchLabel: buildLaunchLabel(app),
  };
}

function buildUnsupportedCapabilityView(capability: Capability): UnsupportedCapabilityView {
  return {
    feature: humanizeToken(capability.feature),
    platform: humanizePlatform(capability.platform),
    reason: capability.reason?.trim() || "No reason provided by server.",
  };
}

function buildLaunchLabel(app: AppSummary): string {
  if (!app.launch) {
    return "Attach to running app";
  }

  if (app.launch.kind === "macosBundle") {
    return "Launch macOS app";
  }

  return `Launch ${commandName(app.launch.value)}`;
}

function buildAppIconView(app: AppSummary): AppIconView {
  const title = app.icon?.source ?? app.name;

  if (app.icon?.dataUrl && isWebRenderableImageMime(app.icon.mimeType)) {
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

function isWebRenderableImageMime(mimeType: string): boolean {
  return ["image/png", "image/jpeg", "image/gif", "image/webp", "image/svg+xml"].includes(
    mimeType.toLowerCase()
  );
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

function commandName(command: string): string {
  const firstToken = command.trim().split(/\s+/)[0] ?? "";
  const parts = firstToken.split("/").filter((part) => part.length > 0);

  return parts.at(-1) ?? appFallbackLabel(command);
}

function appFallbackLabel(value: string): string {
  return value.trim() || "application";
}

function humanizeToken(value: string): string {
  const words = value
    .trim()
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .split(/[\s_-]+/)
    .filter((word) => word.length > 0);

  if (words.length === 0) {
    return "Unknown";
  }

  return words
    .map((word) => `${word[0].toUpperCase()}${word.slice(1).toLowerCase()}`)
    .join(" ");
}

function humanizePlatform(value: string): string {
  const knownPlatformLabels: Record<string, string> = {
    android: "Android",
    ios: "iOS",
    linux: "Linux",
    macos: "macOS",
    windows: "Windows",
  };

  const normalized = value.trim().toLowerCase();
  return knownPlatformLabels[normalized] ?? humanizeToken(value);
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
