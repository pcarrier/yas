import { describe, expect, it, vi } from "vitest";

import {
  YAS_FAMILY_FS,
  YAS_FAMILY_LSP,
  YAS_FS_VERSION,
  YAS_LSP_VERSION,
  YAS_STATUS_UNSUPPORTED,
  YasNativeProductFamilies,
  YasResultError,
} from "../yas";
import type { YasConnection } from "../yas";
import type { YasLspStopServer } from "../yas";

function connection(selected: Set<number>): YasConnection {
  return {
    family(family: number, version = 1) {
      if (family !== YAS_FAMILY_FS || version !== YAS_FS_VERSION)
        throw new YasResultError(YAS_STATUS_UNSUPPORTED, new Uint8Array());
      if (!selected.has(family))
        throw new YasResultError(YAS_STATUS_UNSUPPORTED, new Uint8Array());
      return { family, version };
    },
  } as YasConnection;
}

describe("YasNativeProductFamilies", () => {
  it("constructs an exact typed family lazily and retains its native client", () => {
    const selected = new Set<number>();
    const nativeFs = { marker: "opaque native FS client" };
    const factory = vi.fn(() => nativeFs);
    const families = new YasNativeProductFamilies(connection(selected), {
      fs: factory as never,
    });

    expect(families.fs).toBeNull();
    expect(factory).not.toHaveBeenCalled();

    selected.add(YAS_FAMILY_FS);
    expect(families.fs).toBe(nativeFs);
    expect(families.fs).toBe(nativeFs);
    expect(factory).toHaveBeenCalledOnce();
  });

  it("does not hide protocol failures as family absence", () => {
    const failed = {
      family() {
        throw new Error("corrupt family descriptor");
      },
    } as unknown as YasConnection;
    const families = new YasNativeProductFamilies(failed);
    expect(() => families.supports("fs")).toThrow(/corrupt/);
  });

  it("forwards opaque native handles without allocating browser aliases", async () => {
    const stopServer = vi.fn(
      async (_value: YasLspStopServer) => new Uint8Array(),
    );
    const nativeLsp = { stopServer };
    const selected = {
      family(family: number, version: number) {
        expect([family, version]).toEqual([YAS_FAMILY_LSP, YAS_LSP_VERSION]);
        return { family, version };
      },
    } as unknown as YasConnection;
    const families = new YasNativeProductFamilies(selected, {
      lsp: (() => nativeLsp) as never,
    });
    const identity = {
      operationId: Uint8Array.from({ length: 16 }, (_, index) => index + 1),
      serverHandle: 0xffff_ffff_ffff_fffen,
      generation: 0x1234_5678_9abc_def0n,
    };

    await families.stopLspServer(identity);

    expect(stopServer).toHaveBeenCalledWith(identity);
    const called = stopServer.mock.calls[0]![0];
    expect(typeof called.serverHandle).toBe("bigint");
    expect(typeof called.generation).toBe("bigint");
  });

  it("rejects use after disposal", () => {
    const selected = new Set([YAS_FAMILY_FS]);
    const families = new YasNativeProductFamilies(connection(selected), {
      fs: (() => ({})) as never,
    });
    expect(families.fs).not.toBeNull();
    families.dispose();
    expect(families.fs).toBeNull();
    expect(() => families.require("fs")).toThrow(/disposed/);
  });

  it("disposes every lazily cached disposable family client", () => {
    const disposeFs = vi.fn();
    const disposeLsp = vi.fn();
    const selected = {
      family(family: number, version: number) {
        if (
          (family === YAS_FAMILY_FS && version === YAS_FS_VERSION) ||
          (family === YAS_FAMILY_LSP && version === YAS_LSP_VERSION)
        )
          return { family, version };
        throw new YasResultError(YAS_STATUS_UNSUPPORTED, new Uint8Array());
      },
    } as unknown as YasConnection;
    const families = new YasNativeProductFamilies(selected, {
      fs: (() => ({ dispose: disposeFs })) as never,
      lsp: (() => ({ dispose: disposeLsp })) as never,
    });

    expect(families.fs).not.toBeNull();
    expect(families.lsp).not.toBeNull();
    families.dispose();
    families.dispose();

    expect(disposeFs).toHaveBeenCalledOnce();
    expect(disposeLsp).toHaveBeenCalledOnce();
  });
});
