import { describe, expect, it } from "vitest";
import { requestRoute } from "../app/utils/routes";

describe("requestRoute", () => {
  it("sends a draft to the registration screen with its id", () => {
    expect(requestRoute({ id: "req_1", lane: "draft" })).toBe("/requests/new?id=req_1");
    expect(requestRoute({ id: "a b/c", lane: "draft" })).toBe("/requests/new?id=a%20b%2Fc");
  });
  it("sends every other lane to the request status", () => {
    for (const lane of ["todo", "prepare", "doing", "done", "archive", "Draft", "drafts", ""]) {
      expect(requestRoute({ id: "req_2", lane })).toBe("/requests/req_2");
    }
    expect(requestRoute({ id: "a b/c", lane: "doing" })).toBe("/requests/a%20b%2Fc");
  });
});
