import { beforeEach, describe, expect, it } from "vitest";
import {
  LocalStorageConnectionProfileStore,
  type ConnectionProfile,
  validateConnectionProfile
} from "./connectionProfiles";

const profile: ConnectionProfile = {
  id: "local",
  label: "Local workstation",
  sshUser: "biplab",
  sshHost: "workstation.local",
  localPort: 7676,
  remotePort: 7676,
  authToken: "token"
};

describe("LocalStorageConnectionProfileStore", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("starts empty", () => {
    const store = new LocalStorageConnectionProfileStore(localStorage);

    expect(store.list()).toEqual([]);
  });

  it("ignores corrupted stored data", () => {
    localStorage.setItem("swavan.apprelay.profiles", "{");
    const store = new LocalStorageConnectionProfileStore(localStorage);

    expect(store.list()).toEqual([]);
  });

  it("saves profiles sorted by label", () => {
    const store = new LocalStorageConnectionProfileStore(localStorage);

    store.save({ ...profile, id: "z", label: "Zed" });
    store.save({ ...profile, id: "a", label: "Alpha" });

    expect(store.list().map((savedProfile) => savedProfile.label)).toEqual(["Alpha", "Zed"]);
  });

  it("replaces profiles with the same id", () => {
    const store = new LocalStorageConnectionProfileStore(localStorage);

    store.save(profile);
    store.save({ ...profile, label: "Updated workstation" });

    expect(store.list()).toEqual([{ ...profile, label: "Updated workstation" }]);
  });

  it("removes a profile by id", () => {
    const store = new LocalStorageConnectionProfileStore(localStorage);

    store.save(profile);
    store.remove(profile.id);

    expect(store.list()).toEqual([]);
  });
});

describe("validateConnectionProfile", () => {
  it("rejects missing auth tokens", () => {
    expect(() => validateConnectionProfile({ ...profile, authToken: " " })).toThrow(
      "auth token is required"
    );
  });

  it("rejects invalid ports", () => {
    expect(() => validateConnectionProfile({ ...profile, localPort: 0 })).toThrow(
      "ssh tunnel ports must be between 1 and 65535"
    );
  });
});
