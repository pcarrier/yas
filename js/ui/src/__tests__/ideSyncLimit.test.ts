import { describe, expect, it } from "vitest";
import { YasResultError, YAS_STATUS_RESOURCE_EXHAUSTED } from "@yas-run/core";
import { isSyncLimitError } from "../ide/syncErrors";

describe("IDE filesystem capacity errors", () => {
  it("recognizes the typed resource-exhausted status", () => {
    expect(
      isSyncLimitError(
        new YasResultError(YAS_STATUS_RESOURCE_EXHAUSTED, new Uint8Array()),
      ),
    ).toBe(true);
  });

  it("does not retry unrelated request failures", () => {
    expect(isSyncLimitError(new YasResultError(5, new Uint8Array()))).toBe(
      false,
    );
  });
});
