import { expect, it } from "vitest";
import { toCurl } from "./curl";

it("builds a redacted curl command", () => {
  const command = toCurl({
    method: "POST",
    url: "http://127.0.0.1:1234/v1/chat/completions",
    headers: { Authorization: "Bearer sk-l••••" },
    body: { model: "demo", stream: false },
  });
  expect(command).toContain("curl -X POST");
  expect(command).toContain("Bearer sk-l••••");
  expect(command).not.toContain("sk-live");
  expect(command).toContain("chat/completions");
});

it("builds a powershell style curl command", () => {
  const command = toCurl(
    {
      method: "POST",
      url: "http://127.0.0.1:1234/v1/chat/completions",
      headers: { Authorization: "Bearer sk-l••••" },
      body: { model: "demo", stream: false },
    },
    "powershell",
  );
  expect(command).toContain("curl.exe -X POST");
  expect(command).toContain("`\n");
});
