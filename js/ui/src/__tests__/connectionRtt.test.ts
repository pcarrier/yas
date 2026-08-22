import { describe, expect, it } from "vitest";
import { connectionRttSummary } from "../connectionRtt";

describe("status bar connection RTT", () => {
  it("shows one latency number for one server", () => {
    expect(
      connectionRttSummary([{ status: "connected", rttMs: 4.4 }])?.text,
    ).toBe("4 ms");
  });

  it("keeps two latency numbers for multiple servers with equal RTTs", () => {
    expect(
      connectionRttSummary([
        { status: "connected", rttMs: 4.4 },
        { status: "connected", rttMs: 4.4 },
      ])?.text,
    ).toBe("4–4 ms");
  });

  it("shows the minimum and maximum when several server RTTs differ", () => {
    expect(
      connectionRttSummary([
        { status: "connected", rttMs: 8.2 },
        { status: "connected", rttMs: 2.6 },
        { status: "connected", rttMs: 5.1 },
      ])?.text,
    ).toBe("3–8 ms");
  });

  it("switches to seconds with two decimal places at one second", () => {
    const summary = connectionRttSummary([
      { status: "connected", rttMs: 1_230 },
    ]);
    expect(summary).toMatchObject({
      minimumText: "1.23",
      maximumText: "1.23",
      unit: "s",
      text: "1.23 s",
    });
  });

  it("uses one unit for a range that reaches one second", () => {
    expect(
      connectionRttSummary([
        { status: "connected", rttMs: 250 },
        { status: "connected", rttMs: 1_230 },
      ])?.text,
    ).toBe("0.25–1.23 s");
  });
});
