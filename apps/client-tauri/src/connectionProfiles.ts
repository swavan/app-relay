export type ConnectionProfile = {
  id: string;
  label: string;
  sshUser: string;
  sshHost: string;
  localPort: number;
  remotePort: number;
  authToken: string;
};

export interface ConnectionProfileStore {
  list(): ConnectionProfile[];
  save(profile: ConnectionProfile): void;
  remove(id: string): void;
}

export class LocalStorageConnectionProfileStore implements ConnectionProfileStore {
  constructor(private readonly storage: Storage, private readonly key = "swavan.apprelay.profiles") {}

  list(): ConnectionProfile[] {
    const rawProfiles = this.storage.getItem(this.key);
    if (rawProfiles === null) {
      return [];
    }

    return decodeStoredList(rawProfiles, isConnectionProfile);
  }

  save(profile: ConnectionProfile): void {
    validateConnectionProfile(profile);
    const profiles = this.list().filter((existing) => existing.id !== profile.id);
    profiles.push(profile);
    profiles.sort((left, right) => left.label.localeCompare(right.label));
    this.storage.setItem(this.key, JSON.stringify(profiles));
  }

  remove(id: string): void {
    const profiles = this.list().filter((profile) => profile.id !== id);
    this.storage.setItem(this.key, JSON.stringify(profiles));
  }
}

export function validateConnectionProfile(profile: ConnectionProfile): void {
  if (isBlank(profile.id)) {
    throw new Error("connection profile id is required");
  }

  if (isBlank(profile.label)) {
    throw new Error("connection profile label is required");
  }

  if (isBlank(profile.sshUser)) {
    throw new Error("ssh user is required");
  }

  if (isBlank(profile.sshHost)) {
    throw new Error("ssh host is required");
  }

  if (!isValidPort(profile.localPort) || !isValidPort(profile.remotePort)) {
    throw new Error("ssh tunnel ports must be between 1 and 65535");
  }

  if (isBlank(profile.authToken)) {
    throw new Error("auth token is required");
  }
}

function isConnectionProfile(value: unknown): value is ConnectionProfile {
  if (typeof value !== "object" || value === null) {
    return false;
  }

  const profile = value as ConnectionProfile;
  return (
    typeof profile.id === "string" &&
    typeof profile.label === "string" &&
    typeof profile.sshUser === "string" &&
    typeof profile.sshHost === "string" &&
    typeof profile.localPort === "number" &&
    typeof profile.remotePort === "number" &&
    typeof profile.authToken === "string"
  );
}

function decodeStoredList<T>(rawValue: string, isItem: (value: unknown) => value is T): T[] {
  let items: T[];
  try {
    items = JSON.parse(rawValue) as T[];
  } catch {
    return [];
  }

  if (!Array.isArray(items)) {
    return [];
  }

  return items.filter(isItem);
}

function isValidPort(port: number): boolean {
  return Number.isInteger(port) && port >= 1 && port <= 65_535;
}

function isBlank(value: string): boolean {
  return value.trim().length === 0;
}
