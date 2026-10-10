import type { RawHttp } from "../types";

export function toCurl(
  raw: RawHttp,
  style: "bash" | "powershell" = "bash",
): string {
  if (style === "powershell") {
    const lines = [
      `curl.exe -X ${raw.method} "${raw.url.replace(/"/g, '`"')}"`,
    ];
    for (const [key, value] of Object.entries(raw.headers ?? {})) {
      lines.push(`  -H "${`${key}: ${value}`.replace(/"/g, '`"')}"`);
    }
    if (raw.body !== undefined) {
      lines.push(`  --data "${JSON.stringify(raw.body).replace(/"/g, '`"')}"`);
    }
    return lines.join(" `\n");
  }

  const lines = [`curl -X ${raw.method} ${shellQuote(raw.url)}`];
  for (const [key, value] of Object.entries(raw.headers ?? {})) {
    lines.push(`  -H ${shellQuote(`${key}: ${value}`)}`);
  }
  if (raw.body !== undefined) {
    lines.push(`  --data ${shellQuote(JSON.stringify(raw.body))}`);
  }
  return lines.join(" \\\n");
}

function shellQuote(value: string): string {
  return `'${value.split("'").join("'\\''")}'`;
}
