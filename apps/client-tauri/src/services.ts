import { invoke } from "@tauri-apps/api/core";

export type HealthStatus = {
  service: string;
  healthy: boolean;
  version: string;
};

export type Capability = {
  platform: string;
  feature: string;
  supported: boolean;
  reason?: string;
};

export type AppSummary = {
  id: string;
  name: string;
  icon?: {
    mimeType: string;
    source?: string;
  };
  launch?: {
    kind: "desktopCommand" | "macosBundle";
    value: string;
  };
};

export type ViewportSize = {
  width: number;
  height: number;
};

export type ApplicationSession = {
  id: string;
  applicationId: string;
  selectedWindow: {
    id: string;
    title: string;
  };
  launchIntent?: {
    sessionId: string;
    applicationId: string;
    launch?: {
      kind: "desktopCommand" | "macosBundle";
      value: string;
    };
    status: "recorded" | "attached" | "unsupported";
  };
  viewport: ViewportSize;
  resizeIntent?: {
    sessionId: string;
    selectedWindowId: string;
    viewport: ViewportSize;
    status: "recorded" | "applied" | "unsupported";
  };
  state: "starting" | "ready" | "closed";
};

export interface RemoteService {
  health(): Promise<HealthStatus>;
  capabilities(): Promise<Capability[]>;
  applications(): Promise<AppSummary[]>;
  createSession(applicationId: string, viewport: ViewportSize): Promise<ApplicationSession>;
  resizeSession(sessionId: string, viewport: ViewportSize): Promise<ApplicationSession>;
  closeSession(sessionId: string): Promise<ApplicationSession>;
}

export class TauriRemoteService implements RemoteService {
  private readonly authToken: string;

  constructor(authToken?: string) {
    this.authToken = authToken ?? "local-dev-token";
  }

  async health(): Promise<HealthStatus> {
    return invoke<HealthStatus>("server_health", { authToken: this.authToken });
  }

  async capabilities(): Promise<Capability[]> {
    return invoke<Capability[]>("server_capabilities", { authToken: this.authToken });
  }

  async applications(): Promise<AppSummary[]> {
    return invoke<AppSummary[]>("server_applications", { authToken: this.authToken });
  }

  async createSession(
    applicationId: string,
    viewport: ViewportSize
  ): Promise<ApplicationSession> {
    return invoke<ApplicationSession>("create_application_session", {
      authToken: this.authToken,
      request: { applicationId, viewport }
    });
  }

  async resizeSession(sessionId: string, viewport: ViewportSize): Promise<ApplicationSession> {
    return invoke<ApplicationSession>("resize_application_session", {
      authToken: this.authToken,
      request: { sessionId, viewport }
    });
  }

  async closeSession(sessionId: string): Promise<ApplicationSession> {
    return invoke<ApplicationSession>("close_application_session", {
      authToken: this.authToken,
      sessionId
    });
  }
}
