import { describe, expect, it } from "vitest";
import {
  formatClock,
  formatSegmentTimestamps,
  parseWhisperOutput,
  toSrt,
  toVtt,
  toWordTimestamps,
} from "./subtitles";

describe("subtitles utility", () => {
  it("formats clock with comma and dot", () => {
    expect(formatClock(0, ",")).toBe("00:00:00,000");
    expect(formatClock(65.123, ",")).toBe("00:01:05,123");
    expect(formatClock(3661.05, ".")).toBe("01:01:01.050");
  });

  it("parses whisper verbose json and generates SRT and VTT", () => {
    const raw = {
      text: "Hello world this is a test",
      language: "english",
      duration: 5.5,
      segments: [
        { id: 1, start: 0.0, end: 2.1, text: "Hello world", speaker: "Alice" },
        { id: 2, start: 2.1, end: 5.5, text: "this is a test", speaker: "Bob" },
      ],
      words: [
        { word: "Hello", start: 0.0, end: 1.0 },
        { word: "world", start: 1.0, end: 2.1 },
      ],
    };

    const parsed = parseWhisperOutput(raw);
    expect(parsed.segments.length).toBe(2);
    expect(parsed.words.length).toBe(2);
    expect(parsed.language).toBe("english");

    const srt = toSrt(parsed.segments);
    expect(srt).toContain(
      "1\n00:00:00,000 --> 00:00:02,100\n[Alice] Hello world",
    );
    expect(srt).toContain(
      "2\n00:00:02,100 --> 00:00:05,500\n[Bob] this is a test",
    );

    const vtt = toVtt(parsed.segments);
    expect(vtt).toContain("WEBVTT");
    expect(vtt).toContain(
      "00:00:00.000 --> 00:00:02.100\n<v Alice>Hello world</v>",
    );

    const timestamps = formatSegmentTimestamps(parsed.segments);
    expect(timestamps).toContain(
      "[00:00:00.000 --> 00:00:02.100] Alice: Hello world",
    );

    const words = toWordTimestamps(parsed.words);
    expect(words).toContain("[00:00:00.000 - 00:00:01.000]  Hello");
    expect(words).toContain("[00:00:01.000 - 00:00:02.100]  world");
  });
});
