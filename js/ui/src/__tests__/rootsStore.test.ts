import { describe, expect, it } from "vitest";
import type { ConnectionId } from "@yas-run/core";
import {
  parseRootDocument,
  ROOT_CACHE_MAX_DOCUMENT_BYTES,
  ROOT_CACHE_MAX_ITEMS,
} from "../ide/rootsStore";

const LOCAL = "local" as ConnectionId;

describe("server root documents", () => {
  it("parses enabled and disabled roots under the owning connection", () => {
    expect(
      parseRootDocument("repo = /src/yas\n# data = /srv/data:a", LOCAL),
    ).toEqual([
      {
        name: "repo",
        remote: LOCAL,
        path: "/src/yas",
        disabled: false,
      },
      {
        name: "data",
        remote: LOCAL,
        path: "/srv/data:a",
        disabled: true,
      },
    ]);
  });

  it("bounds hostile document rotation before building UI caches", () => {
    const lines = Array.from(
      { length: ROOT_CACHE_MAX_ITEMS + 100 },
      (_, index) => `root-${index} = /srv/${index}`,
    ).join("\n");
    expect(parseRootDocument(lines, LOCAL)).toHaveLength(ROOT_CACHE_MAX_ITEMS);
    expect(
      parseRootDocument("x".repeat(ROOT_CACHE_MAX_DOCUMENT_BYTES + 1), LOCAL),
    ).toEqual([]);
  });
});
