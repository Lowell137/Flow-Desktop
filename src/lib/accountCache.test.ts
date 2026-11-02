import { describe, expect, it } from "vitest";
import { clearAccountCache, readAccountCache, writeAccountCache } from "./accountCache";

describe("account cache", () => {
  it("gives back what was written until sign-out clears it", () => {
    writeAccountCache("home", [{ id: "a" }]);
    expect(readAccountCache("home")).toEqual([{ id: "a" }]);
    clearAccountCache();
    expect(readAccountCache("home")).toBeNull();
  });
});
