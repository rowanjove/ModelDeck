import { describe, expect, it } from "vitest";
import { estimateTokens, previewContextTokens } from "./tokens";

describe("token estimate", () => {
  it("counts cjk more heavily than ascii", () => {
    expect(estimateTokens("abcd")).toBe(1);
    expect(estimateTokens("你好")).toBeGreaterThanOrEqual(2);
  });

  it("previews only the recent tail", () => {
    const tokens = previewContextTokens({
      messages: [{ text: "obsolete-topic" }, { text: "current" }],
      strategy: "recent",
      limit: 1,
      systemPrompt: "",
    });
    expect(tokens).toBe(estimateTokens("current") + 4);
  });
});
