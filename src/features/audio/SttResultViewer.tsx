import { useState } from "react";
import {
  downloadSubtitleFile,
  formatSegmentTimestamps,
  parseWhisperOutput,
  toSrt,
  toVtt,
  toWordTimestamps,
} from "../../lib/subtitles";
import type { AudioResult } from "../../types";

type SubtitleTab = "plain" | "segments" | "srt" | "vtt" | "words";

export function SttResultViewer({ result }: { result: AudioResult }) {
  const parsed = parseWhisperOutput(
    result.rawResponse,
    result.transcript ?? "",
  );
  const [tab, setTab] = useState<SubtitleTab>("segments");
  const [copied, setCopied] = useState(false);

  const srtContent = toSrt(parsed.segments);
  const vttContent = toVtt(parsed.segments);
  const segmentContent = formatSegmentTimestamps(parsed.segments);
  const wordContent = toWordTimestamps(parsed.words);

  const currentContent = (() => {
    switch (tab) {
      case "plain":
        return parsed.text;
      case "segments":
        return segmentContent || parsed.text;
      case "srt":
        return srtContent || parsed.text;
      case "vtt":
        return vttContent || parsed.text;
      case "words":
        return wordContent || "没有词级时间戳信息";
      default:
        return parsed.text;
    }
  })();

  async function copyCurrent() {
    try {
      await navigator.clipboard.writeText(currentContent);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // ignore
    }
  }

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "10px",
        background: "var(--bg-secondary, #1a1a1a)",
        padding: "14px",
        borderRadius: "6px",
        border: "1px solid var(--border-color, #2e2e33)",
      }}
    >
      {/* Header and metadata */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          flexWrap: "wrap",
          gap: "8px",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
          <strong style={{ fontSize: "14px" }}> 语音识别转写结果</strong>
          {parsed.language ? (
            <span
              style={{
                fontSize: "11px",
                padding: "2px 6px",
                borderRadius: "10px",
                background: "var(--badge-bg, #3f3f46)",
                color: "var(--badge-text, #e4e4e7)",
              }}
            >
              语言: {parsed.language}
            </span>
          ) : null}
          {parsed.duration ? (
            <span
              style={{
                fontSize: "11px",
                color: "var(--text-muted, #a1a1aa)",
              }}
            >
              音频时长: {parsed.duration.toFixed(1)}s
            </span>
          ) : null}
        </div>

        {/* Export buttons */}
        <div style={{ display: "flex", gap: "6px", flexWrap: "wrap" }}>
          <button
            type="button"
            className="button tiny secondary"
            onClick={copyCurrent}
          >
            {copied ? " 已复制" : " 复制当前内容"}
          </button>
          <button
            type="button"
            className="button tiny"
            disabled={!srtContent}
            onClick={() => downloadSubtitleFile("transcript.srt", srtContent)}
            title="导出 SubRip (.srt) 字幕文件"
          >
            导出 .srt
          </button>
          <button
            type="button"
            className="button tiny"
            disabled={!vttContent}
            onClick={() => downloadSubtitleFile("transcript.vtt", vttContent)}
            title="导出 WebVTT (.vtt) 字幕文件"
          >
            导出 .vtt
          </button>
          <button
            type="button"
            className="button tiny"
            onClick={() => downloadSubtitleFile("transcript.txt", parsed.text)}
            title="导出纯文本 (.txt)"
          >
            导出 .txt
          </button>
          <button
            type="button"
            className="button tiny"
            onClick={() =>
              downloadSubtitleFile(
                "transcription.json",
                JSON.stringify(result.rawResponse, null, 2),
                "application/json",
              )
            }
            title="导出完整 Whisper JSON 数据"
          >
            JSON
          </button>
        </div>
      </div>

      {/* Format Switcher Tabs */}
      <div
        style={{
          display: "flex",
          gap: "6px",
          borderBottom: "1px solid var(--border-color, #333)",
          paddingBottom: "8px",
        }}
      >
        <button
          type="button"
          className={`button tiny ${tab === "segments" ? "primary" : "secondary"}`}
          onClick={() => setTab("segments")}
        >
          ⏱ 时间轴分段 ({parsed.segments.length})
        </button>
        <button
          type="button"
          className={`button tiny ${tab === "plain" ? "primary" : "secondary"}`}
          onClick={() => setTab("plain")}
        >
          纯文本
        </button>
        <button
          type="button"
          className={`button tiny ${tab === "srt" ? "primary" : "secondary"}`}
          onClick={() => setTab("srt")}
        >
          SRT 字幕
        </button>
        <button
          type="button"
          className={`button tiny ${tab === "vtt" ? "primary" : "secondary"}`}
          onClick={() => setTab("vtt")}
        >
          WebVTT 字幕
        </button>
        <button
          type="button"
          className={`button tiny ${tab === "words" ? "primary" : "secondary"}`}
          onClick={() => setTab("words")}
        >
          词级时间戳 ({parsed.words.length})
        </button>
      </div>

      {/* Content Rendering */}
      {tab === "segments" && parsed.segments.length > 0 ? (
        <div
          style={{
            maxHeight: "360px",
            overflowY: "auto",
            display: "flex",
            flexDirection: "column",
            gap: "8px",
            padding: "4px 0",
          }}
        >
          {parsed.segments.map((seg) => (
            <div
              key={seg.id}
              style={{
                display: "flex",
                flexDirection: "column",
                gap: "4px",
                padding: "8px 10px",
                background: "rgba(255,255,255,0.03)",
                borderRadius: "4px",
                borderLeft: "3px solid var(--accent, #6366f1)",
              }}
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  fontSize: "11px",
                  color: "var(--text-muted, #888)",
                }}
              >
                <span style={{ fontFamily: "monospace" }}>
                  {seg.start.toFixed(2)}s {seg.end.toFixed(2)}s
                </span>
                {seg.speaker ? (
                  <span
                    style={{
                      background: "rgba(99, 102, 241, 0.2)",
                      color: "#a5b4fc",
                      padding: "1px 6px",
                      borderRadius: "10px",
                      fontWeight: 600,
                    }}
                  >
                    {seg.speaker}
                  </span>
                ) : null}
              </div>
              <div style={{ fontSize: "13px", lineHeight: "1.5" }}>
                {seg.text}
              </div>
            </div>
          ))}
        </div>
      ) : tab === "words" && parsed.words.length > 0 ? (
        <div
          style={{
            maxHeight: "360px",
            overflowY: "auto",
            display: "flex",
            flexWrap: "wrap",
            gap: "6px",
            padding: "8px 0",
          }}
        >
          {parsed.words.map((w, i) => (
            <span
              key={`${w.word}-${w.start}-${i}`}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: "4px",
                padding: "3px 8px",
                background: "rgba(255,255,255,0.04)",
                borderRadius: "4px",
                fontSize: "12px",
              }}
              title={`${w.start.toFixed(2)}s - ${w.end.toFixed(2)}s`}
            >
              <strong style={{ color: "var(--text-primary, #fff)" }}>
                {w.word}
              </strong>
              <small
                style={{ color: "var(--text-muted, #777)", fontSize: "10px" }}
              >
                {w.start.toFixed(1)}s
              </small>
            </span>
          ))}
        </div>
      ) : (
        <pre
          className="history-output"
          style={{
            margin: 0,
            maxHeight: "360px",
            overflowY: "auto",
            whiteSpace: "pre-wrap",
            lineHeight: "1.6",
            fontSize: "12px",
          }}
        >
          {currentContent}
        </pre>
      )}
    </div>
  );
}
