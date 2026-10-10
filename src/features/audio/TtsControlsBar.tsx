import { useState } from "react";
import type { ParameterDefinition } from "../../types";

const SPEED_PRESETS = [0.75, 1.0, 1.25, 1.5, 2.0];

const EMOTIONS = [
  { id: "neutral", label: "自然平静", promptTag: "[情感:自然平静]" },
  { id: "happy", label: "开心愉悦", promptTag: "[情感:开心愉悦]" },
  { id: "gentle", label: "温柔亲切", promptTag: "[情感:温柔亲切]" },
  { id: "serious", label: "严肃庄重", promptTag: "[情感:严肃庄重]" },
  { id: "excited", label: "激昂兴奋", promptTag: "[情感:激昂兴奋]" },
  { id: "sad", label: "低落悲伤", promptTag: "[情感:低落悲伤]" },
];

const STYLES = [
  { id: "narration", label: "旁白解说", promptTag: "[风格:旁白解说]" },
  { id: "chat", label: "日常会话", promptTag: "[风格:日常会话]" },
  { id: "news", label: "新闻播报", promptTag: "[风格:新闻播报]" },
  { id: "audiobook", label: "有声读物", promptTag: "[风格:有声读物]" },
  { id: "cinema", label: "影视配音", promptTag: "[风格:影视配音]" },
];

const LANGUAGES = [
  { code: "", label: "自动识别" },
  { code: "zh", label: "中文" },
  { code: "en", label: "英语" },
  { code: "ja", label: "日语" },
  { code: "ko", label: "韩语" },
  { code: "zh-yue", label: "粤语" },
  { code: "fr", label: "法语" },
  { code: "de", label: "德语" },
];

export function TtsControlsBar({
  values,
  parameters,
  prompt,
  onChangeValues,
  onChangePrompt,
}: {
  values: Record<string, unknown>;
  parameters: ParameterDefinition[];
  prompt: string;
  onChangeValues: (next: Record<string, unknown>) => void;
  onChangePrompt: (next: string) => void;
}) {
  const currentSpeed = typeof values.speed === "number" ? values.speed : 1.0;
  const currentLanguage =
    typeof values.language === "string" ? values.language : "";
  const currentEmotion =
    typeof values.emotion === "string" ? values.emotion : "";
  const currentStyle = typeof values.style === "string" ? values.style : "";

  const hasSpeedParam = parameters.some((p) => p.key === "speed");
  const hasLangParam = parameters.some((p) => p.key === "language");
  const hasEmotionParam = parameters.some((p) => p.key === "emotion");
  const hasStyleParam = parameters.some((p) => p.key === "style");

  const [customSpeed, setCustomSpeed] = useState(false);

  function applySpeed(speed: number) {
    if (hasSpeedParam) {
      onChangeValues({ ...values, speed });
    } else {
      // Inject or update in prompt
      let next = prompt.replace(/\[语速:[0-9.]+x\]\s*/g, "");
      if (speed !== 1.0) {
        next = `[语速:${speed}x] ${next}`.trim();
      }
      onChangePrompt(next);
      onChangeValues({ ...values, speed });
    }
  }

  function applyEmotion(emotionId: string) {
    const item = EMOTIONS.find((e) => e.id === emotionId);
    if (hasEmotionParam) {
      onChangeValues({ ...values, emotion: emotionId });
    } else if (item) {
      // Remove any existing emotion tags from prompt and add new
      let next = prompt.replace(/\[情感:[^\]]+\]\s*/g, "");
      if (emotionId !== "neutral") {
        next = `${item.promptTag} ${next}`.trim();
      }
      onChangePrompt(next);
      onChangeValues({ ...values, emotion: emotionId });
    }
  }

  function applyStyle(styleId: string) {
    const item = STYLES.find((s) => s.id === styleId);
    if (hasStyleParam) {
      onChangeValues({ ...values, style: styleId });
    } else if (item) {
      let next = prompt.replace(/\[风格:[^\]]+\]\s*/g, "");
      next = `${item.promptTag} ${next}`.trim();
      onChangePrompt(next);
      onChangeValues({ ...values, style: styleId });
    }
  }

  function applyLanguage(code: string) {
    if (hasLangParam) {
      onChangeValues({ ...values, language: code });
    } else {
      onChangeValues({ ...values, language: code });
    }
  }

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "8px",
        padding: "10px 12px",
        background: "var(--bg-secondary, #1a1a1a)",
        borderRadius: "6px",
        border: "1px solid var(--border-color, #333)",
        fontSize: "12px",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          borderBottom: "1px solid var(--border-color, #2a2a2a)",
          paddingBottom: "6px",
        }}
      >
        <span style={{ fontWeight: 600, color: "var(--text-primary, #eee)" }}>
          语音控制预设 (TTS Controls)
        </span>
        <button
          type="button"
          className="button tiny"
          onClick={() => setCustomSpeed(!customSpeed)}
        >
          {customSpeed ? "简易语速" : "精细调节"}
        </button>
      </div>

      {/* Speed Controls */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "8px",
          flexWrap: "wrap",
        }}
      >
        <span style={{ color: "var(--text-muted, #888)", width: "36px" }}>
          语速:
        </span>
        {SPEED_PRESETS.map((speed) => (
          <button
            key={speed}
            type="button"
            className={`button tiny ${currentSpeed === speed ? "primary" : "secondary"}`}
            onClick={() => applySpeed(speed)}
          >
            {speed === 1.0 ? "1.0x (常速)" : `${speed}x`}
          </button>
        ))}
        {customSpeed ? (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: "6px",
              marginLeft: "8px",
            }}
          >
            <input
              type="range"
              min="0.25"
              max="4.0"
              step="0.05"
              value={currentSpeed}
              onChange={(e) => applySpeed(Number.parseFloat(e.target.value))}
              style={{ width: "90px" }}
            />
            <span style={{ minWidth: "36px", fontFamily: "monospace" }}>
              {currentSpeed.toFixed(2)}x
            </span>
          </div>
        ) : null}
      </div>

      {/* Emotion Controls */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "8px",
          flexWrap: "wrap",
        }}
      >
        <span style={{ color: "var(--text-muted, #888)", width: "36px" }}>
          情感:
        </span>
        {EMOTIONS.map((emo) => (
          <button
            key={emo.id}
            type="button"
            className={`button tiny ${currentEmotion === emo.id ? "primary" : "secondary"}`}
            onClick={() => applyEmotion(emo.id)}
          >
            {emo.label}
          </button>
        ))}
      </div>

      {/* Style Controls */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "8px",
          flexWrap: "wrap",
        }}
      >
        <span style={{ color: "var(--text-muted, #888)", width: "36px" }}>
          风格:
        </span>
        {STYLES.map((st) => (
          <button
            key={st.id}
            type="button"
            className={`button tiny ${currentStyle === st.id ? "primary" : "secondary"}`}
            onClick={() => applyStyle(st.id)}
          >
            {st.label}
          </button>
        ))}
      </div>

      {/* Language Controls */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "8px",
          flexWrap: "wrap",
        }}
      >
        <span style={{ color: "var(--text-muted, #888)", width: "36px" }}>
          语言:
        </span>
        {LANGUAGES.map((lang) => (
          <button
            key={lang.code}
            type="button"
            className={`button tiny ${currentLanguage === lang.code ? "primary" : "secondary"}`}
            onClick={() => applyLanguage(lang.code)}
          >
            {lang.label}
          </button>
        ))}
      </div>
    </div>
  );
}
