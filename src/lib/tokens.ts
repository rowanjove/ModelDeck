export function estimateTokens(text: string): number {
  if (!text) {
    return 0;
  }
  let ascii = 0;
  let other = 0;
  for (const character of text) {
    if (character.charCodeAt(0) < 128) {
      ascii += 1;
    } else {
      other += 1;
    }
  }
  return Math.max(1, Math.ceil(ascii / 4) + other);
}

export interface PreviewMessage {
  text: string;
  images?: string[];
}

function messageCost(message: PreviewMessage): number {
  return estimateTokens(message.text) + (message.images?.length ?? 0) * 256 + 4;
}

function fitTokens(
  messages: PreviewMessage[],
  system: string,
  budget: number,
): PreviewMessage[] {
  let used = estimateTokens(system);
  const chosen: PreviewMessage[] = [];
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    const cost = messageCost(message);
    if (chosen.length > 0 && used + cost > budget) {
      break;
    }
    used += cost;
    chosen.push(message);
  }
  return chosen.reverse();
}

export function previewContextTokens(input: {
  messages: PreviewMessage[];
  strategy: string;
  limit: number;
  systemPrompt: string;
  summary?: string | null;
}): number {
  const system = input.systemPrompt.trim();
  let selected = input.messages;
  let summary = "";
  if (input.strategy === "recent" || input.strategy === "summary") {
    const limit = Math.min(200, Math.max(1, input.limit || 1));
    const split = Math.max(0, input.messages.length - limit);
    selected = input.messages.slice(split);
    if (input.strategy === "summary" && split > 0 && input.summary?.trim()) {
      summary = input.summary.trim();
    }
  } else if (input.strategy === "tokens") {
    const budget = Math.min(200_000, Math.max(128, input.limit || 128));
    selected = fitTokens(input.messages, system, budget);
  }
  let tokens = estimateTokens(system) + estimateTokens(summary);
  for (const message of selected) {
    tokens += messageCost(message);
  }
  return tokens;
}
