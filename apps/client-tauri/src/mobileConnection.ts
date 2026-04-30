import type { ConnectionProfile } from "./connectionProfiles";
import {
  TauriRemoteService,
  type AppSummary,
  type ApplicationSession,
  type Capability,
  type HealthStatus,
  type RemoteService
} from "./services";

export type MobileClientTarget = "android" | "ios";

export type MobileTestServerConnection = {
  target: MobileClientTarget;
  profileId: string;
  profileLabel: string;
  health: HealthStatus;
  capabilities: Capability[];
  applications: AppSummary[];
  activeSessions: ApplicationSession[];
};

export type MobileRemoteServiceFactory = (profile: ConnectionProfile) => RemoteService;

export async function connectMobileClientToTestServer(
  target: MobileClientTarget,
  profile: ConnectionProfile,
  createRemoteService: MobileRemoteServiceFactory = createTauriProfileRemoteService
): Promise<MobileTestServerConnection> {
  if (profile.authToken.trim() === "") {
    throw new Error("mobile test server profile must include a control-plane auth token");
  }
  if (profile.id.trim() === "") {
    throw new Error("mobile test server profile must include a stable client id");
  }

  const remote = createRemoteService(profile);
  const health = await remote.health();

  if (!health.healthy) {
    throw new Error(`${profile.label} test server is unhealthy`);
  }

  const capabilities = await remote.capabilities();
  const applications = await remote.applications();
  const activeSessions = await remote.activeSessions();

  return {
    target,
    profileId: profile.id,
    profileLabel: profile.label,
    health,
    capabilities,
    applications,
    activeSessions
  };
}

function createTauriProfileRemoteService(profile: ConnectionProfile): RemoteService {
  return new TauriRemoteService(profile.authToken, profile.id);
}
