import { describe, expect, it } from "vitest";
import { placeApplicationSection } from "../switcherSections";

describe("switcher application placement", () => {
  it("makes a matching desktop entry the Enter target", () => {
    const sections = ["matching surface", "actions"];
    placeApplicationSection(sections, "applications", true);
    expect(sections).toEqual(["applications", "matching surface", "actions"]);
  });

  it("keeps the unfiltered catalog at the bottom", () => {
    const sections = ["terminals", "actions"];
    placeApplicationSection(sections, "applications", false);
    expect(sections).toEqual(["terminals", "actions", "applications"]);
  });
});
