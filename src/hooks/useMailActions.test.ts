import { describe, expect, it } from "vitest";
import { withoutThreads } from "./useMailActions";

describe("withoutThreads", () => {
  it("removes a review item and updates every loaded page total immediately", () => {
    const result = withoutThreads({
      pages: [
        { items: [{ thread_id: "first" }], total: 101, next_offset: 50 },
        { items: [{ thread_id: "target" }], total: 101, next_offset: null },
      ],
      pageParams: [0, 50],
    }, new Set(["target"])) as { pages: Array<{ items: Array<{ thread_id: string }>; total: number }> };

    expect(result.pages[0]).toMatchObject({ items: [{ thread_id: "first" }], total: 100 });
    expect(result.pages[1]).toMatchObject({ items: [], total: 100 });
  });
});
