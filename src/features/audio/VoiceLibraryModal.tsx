import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../../api";
import { AudioThumb } from "../../components/AudioThumb";
import { DialogFrame } from "../../components/DialogFrame";
import { ErrorNote } from "../../components/ErrorNote";
import { chooseFiles } from "../../lib/files";
import { fileName } from "../../lib/schema";
import type { AppError, Voice } from "../../types";

export function VoiceLibraryModal({
  voices,
  activeVoiceId,
  onSelectVoice,
  onClose,
}: {
  voices: Voice[];
  activeVoiceId?: string | null;
  onSelectVoice: (voice: Voice) => void;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [filterLang, setFilterLang] = useState("");
  const [editingVoice, setEditingVoice] = useState<Voice | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  // Form states
  const [name, setName] = useState("");
  const [sourcePath, setSourcePath] = useState("");
  const [referencePaths, setReferencePaths] = useState<string[]>([]);
  const [transcript, setTranscript] = useState("");
  const [tags, setTags] = useState("");
  const [language, setLanguage] = useState("");
  const [notes, setNotes] = useState("");

  const saveMutation = useMutation({
    mutationFn: async () => {
      if (editingVoice) {
        return api.updateVoice(editingVoice.id, {
          name: name.trim(),
          sourcePath: sourcePath || null,
          transcript: transcript.trim(),
          tags: tags.trim(),
          language: language.trim() || undefined,
          notes: notes.trim() || undefined,
          referencePaths:
            referencePaths.length > 0 ? referencePaths : undefined,
        });
      }
      return api.createVoice({
        name: name.trim(),
        sourcePath,
        transcript: transcript.trim(),
        tags: tags.trim(),
        language: language.trim() || undefined,
        notes: notes.trim() || undefined,
        referencePaths: referencePaths.length > 0 ? referencePaths : undefined,
      });
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["voices"] });
      resetForm();
    },
    onError: (err) => setError(asAppError(err)),
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteVoice(id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["voices"] });
    },
    onError: (err) => setError(asAppError(err)),
  });

  function startCreate() {
    setIsCreating(true);
    setEditingVoice(null);
    setName("");
    setSourcePath("");
    setReferencePaths([]);
    setTranscript("");
    setTags("");
    setLanguage("zh");
    setNotes("");
    setError(null);
  }

  function startEdit(voice: Voice) {
    setIsCreating(false);
    setEditingVoice(voice);
    setName(voice.name);
    setSourcePath(voice.filePath);
    setReferencePaths(voice.referenceFiles ?? []);
    setTranscript(voice.transcript);
    setTags(voice.tags);
    setLanguage(voice.language ?? "");
    setNotes(voice.notes ?? "");
    setError(null);
  }

  function resetForm() {
    setIsCreating(false);
    setEditingVoice(null);
    setError(null);
  }

  const filtered = voices.filter((v) => {
    if (filterLang && (v.language ?? "") !== filterLang) {
      return false;
    }
    if (search.trim()) {
      const q = search.toLowerCase();
      const matchName = v.name.toLowerCase().includes(q);
      const matchTags = v.tags.toLowerCase().includes(q);
      const matchNotes = (v.notes ?? "").toLowerCase().includes(q);
      const matchTranscript = v.transcript.toLowerCase().includes(q);
      return matchName || matchTags || matchNotes || matchTranscript;
    }
    return true;
  });

  return (
    <DialogFrame label="声音库" onClose={onClose}>
      <div
        style={{
          width: "880px",
          maxWidth: "95vw",
          maxHeight: "90vh",
          backgroundColor: "var(--bg-primary, #18181b)",
          border: "1px solid var(--border-color, #27272a)",
          borderRadius: "8px",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          boxShadow: "var(--shadow-lg)",
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: "16px 20px",
            borderBottom: "1px solid var(--border-color, #27272a)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <div>
            <h2 style={{ margin: 0, fontSize: "18px" }}>
              声音库 (Voice Library 2.0)
            </h2>
            <p
              className="muted"
              style={{ margin: "4px 0 0", fontSize: "12px" }}
            >
              管理参考音频、多样本、音色特征、语言标签及转写文本
            </p>
          </div>
          <div style={{ display: "flex", gap: "8px" }}>
            <button
              type="button"
              className="button primary small"
              onClick={startCreate}
            >
              + 新建声音
            </button>
            <button
              type="button"
              className="button secondary small"
              onClick={onClose}
            >
              关闭
            </button>
          </div>
        </div>

        {/* Search & Filter Bar */}
        <div
          style={{
            padding: "10px 20px",
            borderBottom: "1px solid var(--border-color, #27272a)",
            display: "flex",
            gap: "12px",
            alignItems: "center",
            background: "var(--bg-secondary, #202024)",
          }}
        >
          <input
            style={{ flex: 1 }}
            placeholder="搜索音色名称、标签、语言或转写内容…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <select
            value={filterLang}
            onChange={(e) => setFilterLang(e.target.value)}
            style={{ width: "120px" }}
          >
            <option value="">全部语言</option>
            <option value="zh">中文</option>
            <option value="en">英语</option>
            <option value="ja">日语</option>
            <option value="ko">韩语</option>
            <option value="zh-yue">粤语</option>
          </select>
        </div>

        {/* Body Split */}
        <div
          className="voice-library-body"
          style={{
            display: "flex",
            flex: 1,
            overflow: "hidden",
          }}
        >
          {/* Voice List */}
          <div
            style={{
              flex: 1,
              overflowY: "auto",
              padding: "16px",
              display: "flex",
              flexDirection: "column",
              gap: "10px",
            }}
          >
            {filtered.length === 0 ? (
              <p
                className="muted"
                style={{ textAlign: "center", margin: "40px 0" }}
              >
                没有找到匹配的声音
              </p>
            ) : null}
            {filtered.map((voice) => {
              const isActive = voice.id === activeVoiceId;
              const refCount = 1 + (voice.referenceFiles?.length ?? 0);
              return (
                <div
                  key={voice.id}
                  style={{
                    padding: "12px 14px",
                    background: isActive
                      ? "var(--bg-active, #27272a)"
                      : "var(--bg-secondary, #1f1f23)",
                    border: `1px solid ${
                      isActive
                        ? "var(--accent-color, #6366f1)"
                        : "var(--border-color, #2e2e33)"
                    }`,
                    borderRadius: "6px",
                    display: "flex",
                    flexDirection: "column",
                    gap: "8px",
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                    }}
                  >
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: "8px",
                      }}
                    >
                      <strong style={{ fontSize: "14px" }}>{voice.name}</strong>
                      {voice.language ? (
                        <span
                          style={{
                            fontSize: "10px",
                            padding: "2px 6px",
                            borderRadius: "10px",
                            background: "var(--badge-bg, #3f3f46)",
                            color: "var(--badge-text, #e4e4e7)",
                          }}
                        >
                          {voice.language.toUpperCase()}
                        </span>
                      ) : null}
                      <span
                        style={{
                          fontSize: "11px",
                          color: "var(--text-muted, #a1a1aa)",
                        }}
                      >
                        ({refCount} 个样本)
                      </span>
                    </div>
                    <div style={{ display: "flex", gap: "6px" }}>
                      <button
                        type="button"
                        className="button tiny primary"
                        onClick={() => {
                          onSelectVoice(voice);
                          onClose();
                        }}
                      >
                        应用
                      </button>
                      <button
                        type="button"
                        className="button tiny secondary"
                        onClick={() => startEdit(voice)}
                      >
                        编辑
                      </button>
                      <button
                        type="button"
                        className="button tiny danger"
                        onClick={() => {
                          if (
                            window.confirm(`确定删除声音「${voice.name}」？`)
                          ) {
                            void deleteMutation.mutateAsync(voice.id);
                          }
                        }}
                      >
                        删除
                      </button>
                    </div>
                  </div>

                  {voice.tags ? (
                    <div
                      style={{
                        fontSize: "11px",
                        color: "var(--accent, #818cf8)",
                      }}
                    >
                      {voice.tags}
                    </div>
                  ) : null}

                  {voice.notes ? (
                    <div
                      style={{
                        fontSize: "12px",
                        color: "var(--text-muted, #a1a1aa)",
                      }}
                    >
                      {voice.notes}
                    </div>
                  ) : null}

                  {voice.transcript ? (
                    <div
                      style={{
                        fontSize: "12px",
                        fontStyle: "italic",
                        background: "rgba(0,0,0,0.2)",
                        padding: "4px 8px",
                        borderRadius: "4px",
                      }}
                    >
                      "{voice.transcript}"
                    </div>
                  ) : null}

                  {/* Audio Preview Controls */}
                  <div style={{ marginTop: "4px" }}>
                    <AudioThumb path={voice.filePath} />
                  </div>
                </div>
              );
            })}
          </div>

          {/* Edit / Create Form Panel */}
          {isCreating || editingVoice ? (
            <div
              className="voice-library-editor"
              style={{
                width: "360px",
                borderLeft: "1px solid var(--border-color, #27272a)",
                padding: "16px",
                overflowY: "auto",
                background: "var(--bg-secondary, #1a1a1e)",
                display: "flex",
                flexDirection: "column",
                gap: "12px",
              }}
            >
              <h3 style={{ margin: "0 0 4px", fontSize: "15px" }}>
                {isCreating ? "新建声音" : `编辑声音: ${editingVoice?.name}`}
              </h3>

              <label className="field">
                <span>声音名称 (必填)</span>
                <input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="例如: 暖阳男声、英伦旁白"
                />
              </label>

              <div className="file-list">
                <div className="file-list-head">
                  <span>主参考音频 (必选)</span>
                  <button
                    type="button"
                    className="button tiny"
                    onClick={() => {
                      void chooseFiles("audio", false).then((picked) => {
                        if (picked[0]) setSourcePath(picked[0]);
                      });
                    }}
                  >
                    选择文件
                  </button>
                </div>
                {sourcePath ? (
                  <div style={{ fontSize: "12px" }}>
                    <span>{fileName(sourcePath)}</span>
                    <AudioThumb path={sourcePath} />
                  </div>
                ) : (
                  <p className="muted">未选择</p>
                )}
              </div>

              {/* Multiple Reference Audios */}
              <div className="file-list">
                <div className="file-list-head">
                  <span>附加多样本 ({referencePaths.length})</span>
                  <button
                    type="button"
                    className="button tiny"
                    onClick={() => {
                      void chooseFiles("audio", true).then((picked) => {
                        if (picked.length > 0) {
                          setReferencePaths([...referencePaths, ...picked]);
                        }
                      });
                    }}
                  >
                    + 添加样本
                  </button>
                </div>
                {referencePaths.map((p, idx) => (
                  <div
                    key={p}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      fontSize: "11px",
                      padding: "2px 0",
                    }}
                  >
                    <span title={p}>{fileName(p)}</span>
                    <button
                      type="button"
                      className="button tiny"
                      onClick={() =>
                        setReferencePaths(
                          referencePaths.filter((_, i) => i !== idx),
                        )
                      }
                    >
                      移除
                    </button>
                  </div>
                ))}
              </div>

              <label className="field">
                <span>语言 (Language)</span>
                <select
                  value={language}
                  onChange={(e) => setLanguage(e.target.value)}
                >
                  <option value="">未指定</option>
                  <option value="zh">中文</option>
                  <option value="en">英语</option>
                  <option value="ja">日语</option>
                  <option value="ko">韩语</option>
                  <option value="zh-yue">粤语</option>
                  <option value="fr">法语</option>
                  <option value="de">德语</option>
                </select>
              </label>

              <label className="field">
                <span>标签 (Tags)</span>
                <input
                  value={tags}
                  onChange={(e) => setTags(e.target.value)}
                  placeholder="用逗号分隔，如: 温暖, 男声, 叙事"
                />
              </label>

              <label className="field">
                <span>参考音频转写文本 (Transcript)</span>
                <textarea
                  rows={3}
                  value={transcript}
                  onChange={(e) => setTranscript(e.target.value)}
                  placeholder="参考音频里说的话，提高声音克隆与音色对齐质量"
                />
              </label>

              <label className="field">
                <span>音色备注 / 场景说明 (Notes)</span>
                <textarea
                  rows={2}
                  value={notes}
                  onChange={(e) => setNotes(e.target.value)}
                  placeholder="适用角色、情绪基调或搭配建议"
                />
              </label>

              {error ? <ErrorNote error={error} /> : null}

              <div style={{ display: "flex", gap: "8px", marginTop: "8px" }}>
                <button
                  type="button"
                  className="button primary"
                  disabled={
                    !name.trim() || !sourcePath || saveMutation.isPending
                  }
                  onClick={() => void saveMutation.mutateAsync()}
                >
                  {saveMutation.isPending ? "保存中…" : "保存声音"}
                </button>
                <button
                  type="button"
                  className="button secondary"
                  onClick={resetForm}
                >
                  取消
                </button>
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </DialogFrame>
  );
}
