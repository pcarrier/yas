import { describe, expect, it, vi } from "vitest";
import { PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES } from "../../previewNetProtocol";
import { requestBody } from "../index";

function requestWithBody(
  chunks: readonly Uint8Array[],
  headers: HeadersInit = {},
  cancelled?: () => void,
): Request {
  let index = 0;
  const body = new ReadableStream<Uint8Array>({
    pull(controller) {
      const chunk = chunks[index++];
      if (chunk) controller.enqueue(chunk);
      else controller.close();
    },
    cancel() {
      cancelled?.();
    },
  });
  return {
    method: "POST",
    headers: new Headers(headers),
    clone: () => ({ body }) as Request,
  } as Request;
}

describe("preview request-body admission", () => {
  it("concatenates a bounded streaming request body", async () => {
    const body = await requestBody(
      requestWithBody([new Uint8Array([1, 2]), new Uint8Array([3])]),
    );
    expect(body).toEqual(new Uint8Array([1, 2, 3]));
  });

  it("rejects an oversized declared body before materializing it", async () => {
    const clone = vi.fn(() => {
      throw new Error("must not clone");
    });
    const request = {
      method: "POST",
      headers: new Headers({
        "content-length": String(PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES + 1),
      }),
      clone,
    } as unknown as Request;
    await expect(requestBody(request)).rejects.toThrow(
      /exceeds.*preview limit/,
    );
    expect(clone).not.toHaveBeenCalled();
  });

  it("cancels a hostile streaming body as soon as its byte cap is crossed", async () => {
    let cancelled = false;
    const first = new Uint8Array(PREVIEW_HTTP_MAX_REQUEST_BODY_BYTES);
    const request = requestWithBody([first, new Uint8Array([1])], {}, () => {
      cancelled = true;
    });
    await expect(requestBody(request)).rejects.toThrow(
      /exceeds.*preview limit/,
    );
    expect(cancelled).toBe(true);
  });
});
