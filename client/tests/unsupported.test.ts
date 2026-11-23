import { describe, expect, it } from "vitest";
import { SCREEN_UNSUPPORTED, SERVER_UNSUPPORTED, unsupportedHeadline } from "../app/utils/unsupported";

describe("unsupportedHeadline", () => {
  it("blames the server when nothing else is said", () => {
    expect(unsupportedHeadline("タグと protocol の一覧")).toBe("タグと protocol の一覧: サーバーが未対応です");
    expect(unsupportedHeadline("x", "")).toBe(`x: ${SERVER_UNSUPPORTED}`);
    expect(unsupportedHeadline("x", "  ")).toBe(`x: ${SERVER_UNSUPPORTED}`);
    expect(unsupportedHeadline("x", null)).toBe(`x: ${SERVER_UNSUPPORTED}`);
  });

  it("says what it is given, where the server is not what lacks it", () => {
    const text = unsupportedHeadline("承認・修正の指示・質問への回答", SCREEN_UNSUPPORTED);
    expect(text).toBe("承認・修正の指示・質問への回答: この画面では未対応です");
    expect(text).not.toContain("サーバー");
  });
});
