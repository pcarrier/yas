import { describe, expect, it } from "vitest";
import {
  CookieJar,
  CookieJarStore,
  PREVIEW_COOKIE_JAR_MAX_BYTES,
  PREVIEW_COOKIE_JAR_MAX_ITEMS,
  PREVIEW_COOKIE_MAX_ORIGINS,
  PREVIEW_COOKIE_ORIGIN_KEY_MAX_BYTES,
} from "../cookies";

describe("CookieJar HttpOnly", () => {
  // The preview must not give an app a *weaker* cookie contract than its
  // real origin does. The jar used to ignore HttpOnly entirely while the
  // injected document.cookie shim returned everything in it, so a dev
  // server's HttpOnly session cookie became readable by any script on the
  // page — the whole property the attribute exists to provide.
  it("withholds HttpOnly cookies from script but sends them upstream", () => {
    const jar = new CookieJar();
    jar.set("sid=secret; Path=/; HttpOnly", "/");
    jar.set("theme=dark; Path=/", "/");

    const upstream = jar.header("/app");
    expect(upstream).toContain("sid=secret");
    expect(upstream).toContain("theme=dark");

    const forScript = jar.header("/app", true);
    expect(forScript).toContain("theme=dark");
    expect(forScript).not.toContain("sid");
    expect(forScript).not.toContain("secret");
  });

  it("recognises HttpOnly however it is spelled or ordered", () => {
    for (const header of [
      "a=1; httponly",
      "a=1; HTTPONLY",
      "a=1; HttpOnly; Path=/",
      "a=1; Path=/; HttpOnly",
    ]) {
      const jar = new CookieJar();
      jar.set(header, "/");
      expect(jar.header("/", true), header).toBeUndefined();
      expect(jar.header("/"), header).toBe("a=1");
    }
  });

  it("leaves an all-HttpOnly jar with nothing to say to script", () => {
    const jar = new CookieJar();
    jar.set("only=1; HttpOnly", "/");
    // undefined, not an empty string: the shim distinguishes "no cookies"
    // from a header it should send.
    expect(jar.header("/", true)).toBeUndefined();
  });

  it("does not treat an ordinary cookie as HttpOnly", () => {
    const jar = new CookieJar();
    // A value that merely mentions the word must not trip the parser.
    jar.set("note=httponly; Path=/", "/");
    expect(jar.header("/", true)).toBe("note=httponly");
  });

  it("keeps path scoping and expiry independent of the split", () => {
    const jar = new CookieJar();
    jar.set("deep=1; Path=/admin; HttpOnly", "/");
    jar.set("wide=2; Path=/", "/");
    expect(jar.header("/", true)).toBe("wide=2");
    expect(jar.header("/admin")).toContain("deep=1");
    expect(jar.header("/admin", true)).toBe("wide=2");

    jar.set("gone=3; Path=/; Max-Age=0", "/");
    expect(jar.header("/")).not.toContain("gone");
  });

  it("bounds hostile rotating cookies by item count with deterministic LRU eviction", () => {
    const jar = new CookieJar();
    for (let index = 0; index < PREVIEW_COOKIE_JAR_MAX_ITEMS * 2; index++)
      jar.set(`cookie${index}=x; Path=/p${index}`, "/");

    expect(jar.size).toBe(PREVIEW_COOKIE_JAR_MAX_ITEMS);
    expect(jar.bytes).toBeLessThanOrEqual(PREVIEW_COOKIE_JAR_MAX_BYTES);
    expect(jar.header("/p0")).toBeUndefined();
    expect(jar.header(`/p${PREVIEW_COOKIE_JAR_MAX_ITEMS * 2 - 1}`)).toBe(
      `cookie${PREVIEW_COOKIE_JAR_MAX_ITEMS * 2 - 1}=x`,
    );
  });

  it("enforces the byte cap independently and preserves replacement semantics", () => {
    const jar = new CookieJar(10, 240);
    jar.set(`a=${"x".repeat(40)}; Path=/a`, "/");
    jar.set(`b=${"y".repeat(40)}; Path=/b`, "/");
    expect(jar.bytes).toBeLessThanOrEqual(240);
    expect(jar.size).toBe(1);
    expect(jar.header("/a")).toBeUndefined();
    expect(jar.header("/b")).toBe(`b=${"y".repeat(40)}`);

    jar.set("b=replaced; Path=/b; HttpOnly", "/");
    expect(jar.size).toBe(1);
    expect(jar.header("/b")).toBe("b=replaced");
    expect(jar.header("/b", true)).toBeUndefined();
    jar.set("b=gone; Path=/b; Max-Age=0", "/");
    expect(jar.size).toBe(0);
    expect(jar.bytes).toBe(0);
  });

  it("promotes used paths without changing stable Cookie header order", () => {
    const jar = new CookieJar(3, 10_000);
    jar.set("a=1; Path=/a", "/");
    jar.set("b=2; Path=/b", "/");
    jar.set("c=3; Path=/c", "/");
    expect(jar.header("/a")).toBe("a=1");
    jar.set("d=4; Path=/d", "/");
    expect(jar.header("/b")).toBeUndefined();
    expect(jar.header("/a")).toBe("a=1");

    const ordered = new CookieJar();
    ordered.set("first=1; Path=/", "/");
    ordered.set("second=2; Path=/", "/");
    expect(ordered.header("/")).toBe("first=1; second=2");
    expect(ordered.header("/")).toBe("first=1; second=2");
  });

  it("bounds hostile origin rotation and cleans stale or empty jars", () => {
    const store = new CookieJarStore();
    for (let index = 0; index < PREVIEW_COOKIE_MAX_ORIGINS * 2; index++)
      store.obtain(`origin-${index}`).set("sid=1", "/");
    expect(store.size).toBe(PREVIEW_COOKIE_MAX_ORIGINS);
    expect(store.bytes).toBeLessThanOrEqual(
      PREVIEW_COOKIE_ORIGIN_KEY_MAX_BYTES,
    );
    expect([...store.keys()][0]).toBe(`origin-${PREVIEW_COOKIE_MAX_ORIGINS}`);

    const reused = store.get(`origin-${PREVIEW_COOKIE_MAX_ORIGINS}`)!;
    store.obtain("new-origin").set("sid=2", "/");
    expect(store.get(`origin-${PREVIEW_COOKIE_MAX_ORIGINS}`)).toBe(reused);
    const empty = store.obtain("empty");
    store.deleteIfEmpty("empty", empty);
    expect([...store.keys()]).not.toContain("empty");

    store.retainOnly(new Set([`origin-${PREVIEW_COOKIE_MAX_ORIGINS}`]));
    expect([...store.keys()]).toEqual([`origin-${PREVIEW_COOKIE_MAX_ORIGINS}`]);
  });

  it("bounds retained origin-key bytes and leaves oversized keys ephemeral", () => {
    const store = new CookieJarStore(10, 100);
    const first = store.obtain("small");
    first.set("sid=1", "/");
    expect(store.get("small")).toBe(first);

    const oversized = store.obtain("x".repeat(100));
    oversized.set("sid=2", "/");
    expect(store.size).toBe(1);
    expect(store.get("x".repeat(100))).toBeUndefined();
    expect(store.bytes).toBeLessThanOrEqual(100);
  });
});
