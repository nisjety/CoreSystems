import { describe, expect, it } from "vitest";

import {
  providerKeysForConnectionIdentifier,
  providerKeysForConnectionIdentifiers,
} from "./provider-keys";

describe("providerKeysForConnectionIdentifier", () => {
  it("maps Microsoft source aliases to both active and legacy provider keys", () => {
    expect(providerKeysForConnectionIdentifier("microsoft365")).toEqual([
      "microsoft",
      "microsoft-graph",
    ]);
    expect(providerKeysForConnectionIdentifier("SharePoint")).toEqual([
      "microsoft",
      "microsoft-graph",
    ]);
  });

  it("maps document and tool connectors to integration-core provider keys", () => {
    expect(providerKeysForConnectionIdentifiers(["gdrive", "notion", "github"])).toEqual([
      "google",
      "google-drive",
      "notion",
      "github",
    ]);
  });

  it("drops unsupported identifiers instead of forwarding arbitrary provider keys", () => {
    expect(providerKeysForConnectionIdentifiers(["unknown", "slack", "slack"])).toEqual([
      "slack",
    ]);
  });
});
