export interface SubtitleSegment {
  id: number;
  start: number;
  end: number;
  text: string;
  speaker?: string;
}

export interface SubtitleWord {
  word: string;
  start: number;
  end: number;
}

export interface ParsedSubtitleData {
  text: string;
  segments: SubtitleSegment[];
  words: SubtitleWord[];
  language?: string;
  duration?: number;
}

export function formatClock(seconds: number, delimiter = ","): string {
  const totalMs = Math.round(Math.max(0, seconds) * 1000);
  const ms = totalMs % 1000;
  const totalSeconds = Math.floor(totalMs / 1000);
  const secs = totalSeconds % 60;
  const mins = Math.floor(totalSeconds / 60) % 60;
  const hours = Math.floor(totalSeconds / 3600);
  const pad = (n: number, z = 2) => String(n).padStart(z, "0");
  return `${pad(hours)}:${pad(mins)}:${pad(secs)}${delimiter}${pad(ms, 3)}`;
}

export function parseWhisperOutput(
  raw: unknown,
  fallbackText = "",
): ParsedSubtitleData {
  if (!raw || typeof raw !== "object") {
    return {
      text: fallbackText.trim(),
      segments: [],
      words: [],
    };
  }

  const obj = raw as Record<string, unknown>;
  const text =
    typeof obj.text === "string" ? obj.text.trim() : fallbackText.trim();
  const language = typeof obj.language === "string" ? obj.language : undefined;
  const duration = typeof obj.duration === "number" ? obj.duration : undefined;

  const rawSegments = Array.isArray(obj.segments) ? obj.segments : [];
  const segments: SubtitleSegment[] = [];
  for (let index = 0; index < rawSegments.length; index++) {
    const item = rawSegments[index];
    if (!item || typeof item !== "object") continue;
    const seg = item as Record<string, unknown>;
    const segText = typeof seg.text === "string" ? seg.text.trim() : "";
    if (!segText) continue;
    segments.push({
      id: typeof seg.id === "number" ? seg.id : index + 1,
      start: typeof seg.start === "number" ? seg.start : 0,
      end: typeof seg.end === "number" ? seg.end : 0,
      text: segText,
      speaker: typeof seg.speaker === "string" ? seg.speaker.trim() : undefined,
    });
  }

  const rawWords = Array.isArray(obj.words) ? obj.words : [];
  const words: SubtitleWord[] = [];
  for (const item of rawWords) {
    if (!item || typeof item !== "object") continue;
    const w = item as Record<string, unknown>;
    const word = typeof w.word === "string" ? w.word.trim() : "";
    if (!word) continue;
    words.push({
      word,
      start: typeof w.start === "number" ? w.start : 0,
      end: typeof w.end === "number" ? w.end : 0,
    });
  }

  return {
    text,
    segments,
    words,
    language,
    duration,
  };
}

export function toSrt(segments: SubtitleSegment[]): string {
  if (segments.length === 0) return "";
  return segments
    .map((seg, idx) => {
      const spk = seg.speaker ? `[${seg.speaker}] ` : "";
      return `${idx + 1}\n${formatClock(seg.start, ",")} --> ${formatClock(
        seg.end,
        ",",
      )}\n${spk}${seg.text}`;
    })
    .join("\n\n");
}

export function toVtt(segments: SubtitleSegment[]): string {
  if (segments.length === 0) return "WEBVTT\n";
  const body = segments
    .map((seg) => {
      const text = seg.speaker ? `<v ${seg.speaker}>${seg.text}</v>` : seg.text;
      return `${formatClock(seg.start, ".")} --> ${formatClock(
        seg.end,
        ".",
      )}\n${text}`;
    })
    .join("\n\n");
  return `WEBVTT\n\n${body}`;
}

export function formatSegmentTimestamps(segments: SubtitleSegment[]): string {
  return segments
    .map((seg) => {
      const stamp = `[${formatClock(seg.start, ".")} --> ${formatClock(
        seg.end,
        ".",
      )}]`;
      return seg.speaker
        ? `${stamp} ${seg.speaker}: ${seg.text}`
        : `${stamp} ${seg.text}`;
    })
    .join("\n");
}

export function toWordTimestamps(words: SubtitleWord[]): string {
  return words
    .map(
      (w) =>
        `[${formatClock(w.start, ".")} - ${formatClock(w.end, ".")}]  ${w.word}`,
    )
    .join("\n");
}

export function downloadSubtitleFile(
  filename: string,
  content: string,
  mime = "text/plain;charset=utf-8",
) {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}
