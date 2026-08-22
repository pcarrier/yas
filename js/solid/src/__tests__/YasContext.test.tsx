import { describe, it, expect } from "vitest";
import { render } from "@solidjs/testing-library";
import { YasWorkspaceProvider, useYasContext } from "../YasContext";
import type { YasContextValue } from "../YasContext";
import type { YasWorkspace } from "@yas-run/core";
import type { JSX } from "solid-js";

function renderWithContext(value: YasContextValue) {
  let captured: YasContextValue = {};
  render(() => (
    <YasWorkspaceProvider {...value}>
      {
        (() => {
          captured = useYasContext();
          return null;
        })() as unknown as JSX.Element
      }
    </YasWorkspaceProvider>
  ));
  return captured;
}

describe("YasContext", () => {
  it("returns empty object without provider", () => {
    let captured: YasContextValue = { workspace: {} as YasWorkspace };
    render(() => {
      captured = useYasContext();
      return null;
    });
    expect(captured).toEqual({});
  });

  it("provides workspace", () => {
    const workspace = {} as YasWorkspace;
    const ctx = renderWithContext({ workspace });
    expect(ctx.workspace).toBe(workspace);
  });

  it("provides palette", () => {
    const palette = {
      id: "test",
      name: "Test",
      dark: true,
      fg: [255, 255, 255] as [number, number, number],
      bg: [0, 0, 0] as [number, number, number],
      ansi: Array.from(
        { length: 16 },
        () => [0, 0, 0] as [number, number, number],
      ),
    };
    const ctx = renderWithContext({ palette });
    expect(ctx.palette).toBe(palette);
  });

  it("provides fontFamily and fontSize", () => {
    const ctx = renderWithContext({ fontFamily: "monospace", fontSize: 14 });
    expect(ctx.fontFamily).toBe("monospace");
    expect(ctx.fontSize).toBe(14);
  });

  it("provides undefined for omitted values", () => {
    const ctx = renderWithContext({});
    expect(ctx.workspace).toBeUndefined();
    expect(ctx.palette).toBeUndefined();
    expect(ctx.fontFamily).toBeUndefined();
    expect(ctx.fontSize).toBeUndefined();
  });
});
