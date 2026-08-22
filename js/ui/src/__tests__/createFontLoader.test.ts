import { createRoot, createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  YAS_FONT_FAMILY_MONOSPACE,
  YAS_FONT_FACE_FETCHABLE,
  YAS_FONT_STYLE_ITALIC,
  YAS_FONT_STYLE_NORMAL,
} from "@yas-run/core";
import {
  createFontLoader,
  fontAdvanceRatio,
  fontProtocolSourceKey,
  protocolFontFamilies,
  type FontDescription,
  type FontFaceData,
  type FontProtocolConnection,
  type FontProtocolSource,
} from "../createFontLoader";
import { loadFontFace, saveFontFace } from "../fontStore";

const faceCache = vi.hoisted(() => new Map<string, Uint8Array>());

vi.mock("../fontStore", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../fontStore")>();
  return {
    ...actual,
    loadFontFace: vi.fn(async (hash: string) => {
      const data = faceCache.get(hash);
      return data ? { data: data.slice(), savedAt: 1, usedAt: 1 } : null;
    }),
    saveFontFace: vi.fn(async (hash: string, data: Uint8Array) => {
      faceCache.set(hash, data.slice());
    }),
  };
});

class TestFontFace {
  readonly family: string;
  readonly descriptors: FontFaceDescriptors;

  constructor(
    family: string,
    readonly source: string | BufferSource,
    descriptors: FontFaceDescriptors = {},
  ) {
    this.family = family;
    this.descriptors = descriptors;
  }

  async load(): Promise<TestFontFace> {
    return this;
  }
}

let added: TestFontFace[];
let deleted: TestFontFace[];

function installFontApi(): void {
  added = [];
  deleted = [];
  vi.stubGlobal("FontFace", TestFontFace);
  const fonts = {
    add: vi.fn((face: TestFontFace) => {
      added.push(face);
      return fonts;
    }),
    delete: vi.fn((face: TestFontFace) => {
      deleted.push(face);
      return true;
    }),
    load: vi.fn(async () => []),
    ready: Promise.resolve(undefined),
  };
  Object.defineProperty(document, "fonts", {
    configurable: true,
    value: fonts,
  });
}

function hash(byte: number): Uint8Array {
  return new Uint8Array(32).fill(byte);
}

function description(
  regularHash: Uint8Array = hash(1),
  italicHash: Uint8Array = hash(2),
): FontDescription {
  return {
    handle: 1n,
    generation: 1n,
    descriptionHash: hash(9),
    family: "Test Mono",
    faces: [
      {
        handle: 1n,
        contentHash: regularHash,
        byteLength: 4n,
        format: 0,
        style: YAS_FONT_STYLE_NORMAL,
        flags: YAS_FONT_FACE_FETCHABLE,
        weightMin: 400,
        weightDefault: 400,
        weightMax: 400,
        stretchMin: 100,
        stretchDefault: 100,
        stretchMax: 100,
        slantTenthsDegrees: 0,
        unitsPerEm: 1000,
        cellAdvance: 600,
        ascent: 800,
        descent: -200,
        lineGap: 100,
        subfamily: "Regular",
        postscript: "TestMono-Regular",
        extensions: [],
      },
      {
        handle: 2n,
        contentHash: italicHash,
        byteLength: 4n,
        format: 0,
        style: YAS_FONT_STYLE_ITALIC,
        flags: YAS_FONT_FACE_FETCHABLE,
        weightMin: 700,
        weightDefault: 700,
        weightMax: 700,
        stretchMin: 100,
        stretchDefault: 100,
        stretchMax: 100,
        slantTenthsDegrees: -120,
        unitsPerEm: 1000,
        cellAdvance: 600,
        ascent: 800,
        descent: -200,
        lineGap: 100,
        subfamily: "Bold Italic",
        postscript: "TestMono-BoldItalic",
        extensions: [],
      },
    ],
    extensions: [],
  };
}

function connection(font: FontDescription = description()): {
  connection: FontProtocolConnection;
  listFonts: ReturnType<typeof vi.fn>;
  describeFont: ReturnType<typeof vi.fn>;
  fetchFont: ReturnType<typeof vi.fn>;
} {
  const listFonts = vi.fn(async () => []);
  const describeFont = vi.fn(async () => font);
  const fetchFont = vi.fn(
    async (contentHash: Uint8Array): Promise<FontFaceData> => ({
      contentHash: contentHash.slice(),
      format: 0,
      data: new Uint8Array([contentHash[0], 2, 3, 4]),
    }),
  );
  return {
    connection: { listFonts, describeFont, fetchFont },
    listFonts,
    describeFont,
    fetchFont,
  };
}

function source(
  key: string,
  connection: FontProtocolConnection,
): FontProtocolSource {
  return {
    key,
    connected: true,
    connection,
    hashFont: (data) => hash(data[0]),
  };
}

beforeEach(() => {
  faceCache.clear();
  localStorage.clear();
  document.head.replaceChildren();
  installFontApi();
  vi.stubGlobal("fetch", vi.fn());
});

afterEach(() => {
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

describe("fontAdvanceRatio", () => {
  it("prefers the upright regular fixed-width face", () => {
    const font = description();
    font.faces[0].cellAdvance = 500;
    font.faces[1].cellAdvance = 700;
    expect(fontAdvanceRatio(font)).toBe(0.5);
  });
});

describe("protocolFontFamilies", () => {
  it("keeps monospace DESCRIBE keys rather than display labels", () => {
    expect(
      protocolFontFamilies([
        {
          flags: YAS_FONT_FAMILY_MONOSPACE,
          faceCount: 1,
          family: "opaque-z",
          display: "Readable A",
        },
        {
          flags: 0,
          faceCount: 1,
          family: "proportional",
          display: "Proportional",
        },
        {
          flags: YAS_FONT_FAMILY_MONOSPACE,
          faceCount: 1,
          family: "opaque-a",
          display: "Readable Z",
        },
      ]),
    ).toEqual(["opaque-a", "opaque-z"]);
  });
});

describe("fontProtocolSourceKey", () => {
  it("distinguishes replacement connections with the same id and generation", () => {
    const first = connection().connection;
    const second = connection().connection;
    expect(fontProtocolSourceKey("work", 0, first)).toBe(
      fontProtocolSourceKey("work", 0, first),
    );
    expect(fontProtocolSourceKey("work", 0, first)).not.toBe(
      fontProtocolSourceKey("work", 0, second),
    );
  });
});

describe("createFontLoader FONT protocol", () => {
  it("finishes persisting before readiness and reuses the bytes after remount", async () => {
    const font = description();
    font.faces = [font.faces[0]];
    const first = connection(font);
    const reloaded = connection(font);
    let commit!: () => void;
    const writing = new Promise<void>((resolve) => {
      commit = resolve;
    });
    vi.mocked(saveFontFace).mockImplementationOnce(async (hash, data) => {
      await writing;
      faceCache.set(hash, data.slice());
    });
    let dispose = () => {};
    let loader!: ReturnType<typeof createFontLoader>;
    const mount = (remote: FontProtocolConnection) =>
      createRoot((cleanup) => {
        dispose = cleanup;
        loader = createFontLoader(
          () => "Test Mono",
          "monospace",
          () => source("home", remote),
        );
      });

    try {
      mount(first.connection);
      await vi.waitFor(() => expect(saveFontFace).toHaveBeenCalledOnce());
      expect(loader.fontLoading()).toBe(true);
      commit();
      await vi.waitFor(() => expect(loader.fontLoading()).toBe(false));
      dispose();

      mount(reloaded.connection);
      await vi.waitFor(() => expect(loader.fontLoading()).toBe(false));
      expect(reloaded.describeFont).toHaveBeenCalledOnce();
      expect(reloaded.fetchFont).not.toHaveBeenCalled();
    } finally {
      commit();
      dispose();
    }
  });

  it("still loads fonts when browser cache reads or writes fail", async () => {
    vi.mocked(loadFontFace).mockRejectedValueOnce(new Error("storage closed"));
    vi.mocked(saveFontFace).mockRejectedValueOnce(new Error("quota exceeded"));
    const remote = connection();
    let dispose = () => {};
    let loader!: ReturnType<typeof createFontLoader>;
    createRoot((cleanup) => {
      dispose = cleanup;
      loader = createFontLoader(
        () => "Test Mono",
        "monospace",
        () => source("home", remote.connection),
      );
    });
    try {
      await vi.waitFor(() => expect(loader.fontLoading()).toBe(false));
      expect(remote.fetchFont).toHaveBeenCalledTimes(2);
      expect(added).toHaveLength(2);
    } finally {
      dispose();
    }
  });

  it("fetches exact faces without HTTP and installs their style and weight", async () => {
    const remote = connection();
    const http = vi.mocked(fetch);
    let dispose = () => {};
    let loader!: ReturnType<typeof createFontLoader>;
    createRoot((rootDispose) => {
      dispose = rootDispose;
      loader = createFontLoader(
        () => "Test Mono",
        "monospace",
        () => source("home", remote.connection),
      );
    });

    await vi.waitFor(() => expect(loader.fontLoading()).toBe(false));
    expect(remote.describeFont).toHaveBeenCalledWith("Test Mono");
    expect(remote.fetchFont).toHaveBeenCalledTimes(2);
    expect(http).not.toHaveBeenCalled();
    expect(added.map((face) => face.descriptors)).toEqual([
      { style: "normal", weight: "400" },
      { style: "italic", weight: "700" },
    ]);
    expect(loader.advanceRatio()).toBe(0.6);
    expect(saveFontFace).toHaveBeenCalledTimes(2);
    dispose();
  });

  it("reloads on active-server switch and reuses the global hash cache", async () => {
    const shared = hash(7);
    const onlyRegular = description(shared, hash(8));
    onlyRegular.faces = [onlyRegular.faces[0]];
    const first = connection(onlyRegular);
    const second = connection(onlyRegular);
    const [active, setActive] = createSignal(source("first", first.connection));
    let dispose = () => {};
    let loader!: ReturnType<typeof createFontLoader>;
    createRoot((rootDispose) => {
      dispose = rootDispose;
      loader = createFontLoader(() => "Test Mono", "monospace", active);
    });

    await vi.waitFor(() => expect(first.fetchFont).toHaveBeenCalledTimes(1));
    expect(loadFontFace).toHaveBeenCalled();
    setActive(source("second", second.connection));
    await vi.waitFor(() =>
      expect(second.describeFont).toHaveBeenCalledTimes(1),
    );
    await vi.waitFor(() => expect(added).toHaveLength(2));
    expect(second.fetchFont).not.toHaveBeenCalled();
    expect(deleted).toContain(added[0]);
    expect(loader.advanceRatio()).toBe(0.6);
    expect(fetch).not.toHaveBeenCalled();
    dispose();
  });

  it("retains the selected face when the next server cannot replace it", async () => {
    const onlyRegular = description();
    onlyRegular.faces = [onlyRegular.faces[0]];
    const home = connection(onlyRegular);
    const missing = connection(onlyRegular);
    missing.describeFont.mockRejectedValue(new Error("font not found"));
    const [active, setActive] = createSignal(source("home", home.connection));
    let dispose = () => {};
    let loader!: ReturnType<typeof createFontLoader>;
    createRoot((rootDispose) => {
      dispose = rootDispose;
      loader = createFontLoader(() => "Test Mono", "monospace", active);
    });

    await vi.waitFor(() => expect(added).toHaveLength(1));
    setActive(source("prod", missing.connection));
    await vi.waitFor(() =>
      expect(missing.describeFont).toHaveBeenCalledWith("Test Mono"),
    );
    await vi.waitFor(() => expect(loader.fontLoading()).toBe(false));

    expect(added).toHaveLength(1);
    expect(deleted).toEqual([]);
    expect(loader.resolvedFont()).toBe("Test Mono");
    expect(loader.advanceRatio()).toBe(0.6);

    dispose();
    expect(deleted).toEqual([added[0]]);
  });

  it("does not trust the global cache without a local BLAKE3 verifier", async () => {
    const remote = connection();
    faceCache.set("01".repeat(32), new Uint8Array([1, 2, 3, 4]));
    const unverified: FontProtocolSource = {
      key: "unverified",
      connected: true,
      connection: remote.connection,
    };
    let dispose = () => {};
    let loader!: ReturnType<typeof createFontLoader>;
    createRoot((rootDispose) => {
      dispose = rootDispose;
      loader = createFontLoader(
        () => "Test Mono",
        "monospace",
        () => unverified,
      );
    });

    await vi.waitFor(() => expect(loader.fontLoading()).toBe(false));
    expect(loadFontFace).not.toHaveBeenCalled();
    expect(saveFontFace).not.toHaveBeenCalled();
    expect(remote.fetchFont).toHaveBeenCalledTimes(2);
    dispose();
  });

  it("uses only page-local faces when a connected server lacks FONT", async () => {
    let dispose = () => {};
    let loader!: ReturnType<typeof createFontLoader>;
    createRoot((rootDispose) => {
      dispose = rootDispose;
      loader = createFontLoader(
        () => "Test Mono",
        "monospace",
        () => ({
          key: "no-font",
          connected: true,
          connection: null,
        }),
      );
    });

    await vi.waitFor(() => expect(loader.fontLoading()).toBe(false));
    expect(fetch).not.toHaveBeenCalled();
    expect(loader.advanceRatio()).toBeUndefined();
    expect(document.fonts.load).toHaveBeenCalledWith(
      '16px "Test Mono"',
      "BESbswy",
    );
    dispose();
  });
});
