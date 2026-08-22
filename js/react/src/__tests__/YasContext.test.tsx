import { describe, it, expect } from "vitest";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { YasWorkspaceProvider, useYasContext } from "../YasContext";
import type { YasContextValue } from "../YasContext";
import type { YasWorkspace } from "@yas-run/core";

function wrapper(value: YasContextValue) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <YasWorkspaceProvider {...value}>{children}</YasWorkspaceProvider>;
  };
}

describe("YasContext", () => {
  it("returns empty object without provider", () => {
    const { result } = renderHook(() => useYasContext());
    expect(result.current).toEqual({});
  });

  it("provides workspace", () => {
    const workspace = {} as YasWorkspace;
    const { result } = renderHook(() => useYasContext(), {
      wrapper: wrapper({ workspace }),
    });
    expect(result.current.workspace).toBe(workspace);
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
    const { result } = renderHook(() => useYasContext(), {
      wrapper: wrapper({ palette }),
    });
    expect(result.current.palette).toBe(palette);
  });

  it("provides fontFamily and fontSize", () => {
    const { result } = renderHook(() => useYasContext(), {
      wrapper: wrapper({ fontFamily: "monospace", fontSize: 14 }),
    });
    expect(result.current.fontFamily).toBe("monospace");
    expect(result.current.fontSize).toBe(14);
  });

  it("provides undefined for omitted values", () => {
    const { result } = renderHook(() => useYasContext(), {
      wrapper: wrapper({}),
    });
    expect(result.current.workspace).toBeUndefined();
    expect(result.current.palette).toBeUndefined();
    expect(result.current.fontFamily).toBeUndefined();
    expect(result.current.fontSize).toBeUndefined();
  });
});
