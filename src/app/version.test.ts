import { describe, expect, it } from "vitest";
import pkg from "../../package.json";
import { APP_VERSION, edition, editionName, versionText } from "./version";

describe("version", () => {
  it("is the package version, set at build time", () => {
    expect(APP_VERSION).toBe(pkg.version);
    expect(versionText()).toMatch(new RegExp(`^Version ${pkg.version.replaceAll(".", "\.")}( \(.+\))?$`));
  });
  it("names the edition", () => {
    expect(edition()).toBe("browser"); // no Tauri, no Electron under vitest
    expect(editionName("native")).toBe("Desktop app (native engine)");
    expect(editionName("electron")).toBe("Desktop app (Electron)");
    expect(editionName("browser")).toBe("Browser version");
  });
});
