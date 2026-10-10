import { Suspense, lazy, useEffect, useRef, useState } from "react";
import { TextView } from "../features/text/TextView";

const ImageView = lazy(() =>
  import("../features/image/ImageView").then((m) => ({ default: m.ImageView })),
);
const VideoView = lazy(() =>
  import("../features/video/VideoView").then((m) => ({ default: m.VideoView })),
);
const AudioView = lazy(() =>
  import("../features/audio/AudioView").then((m) => ({ default: m.AudioView })),
);
const PipelineView = lazy(() =>
  import("../features/automation/PipelineView").then((m) => ({
    default: m.PipelineView,
  })),
);
const HistoryView = lazy(() =>
  import("../features/history/HistoryView").then((m) => ({
    default: m.HistoryView,
  })),
);
const AnalyticsView = lazy(() =>
  import("../features/analytics/AnalyticsView").then((m) => ({
    default: m.AnalyticsView,
  })),
);
const DeveloperView = lazy(() =>
  import("../features/developer/DeveloperView").then((m) => ({
    default: m.DeveloperView,
  })),
);
const ModelsView = lazy(() =>
  import("../features/models/ModelsView").then((m) => ({
    default: m.ModelsView,
  })),
);
const SettingsView = lazy(() =>
  import("../features/settings/SettingsView").then((m) => ({
    default: m.SettingsView,
  })),
);
import { api } from "../api";
import { SetupWizardModal } from "../features/setup/SetupWizardModal";
import {
  useAudioDraft,
  useImageDraft,
  useRestore,
  useTextDraft,
  useVideoDraft,
} from "../store";
import { CommandPalette } from "./CommandPalette";
import { ErrorBoundary } from "./ErrorBoundary";

type Route =
  | "text"
  | "image"
  | "video"
  | "audio"
  | "workflow"
  | "history"
  | "analytics"
  | "developer"
  | "models"
  | "settings";

export function Shell() {
  const [route, setRoute] = useState<Route>("text");
  const [fresh, setFresh] = useState(0);
  const [imageFresh, setImageFresh] = useState(0);
  const [videoFresh, setVideoFresh] = useState(0);
  const [audioFresh, setAudioFresh] = useState(0);
  const [switcher, setSwitcher] = useState(false);
  const [showWizard, setShowWizard] = useState(false);
  const reset = useTextDraft((draft) => draft.reset);
  const resetImage = useImageDraft((draft) => draft.reset);
  const resetVideo = useVideoDraft((draft) => draft.reset);
  const resetAudio = useAudioDraft((draft) => draft.reset);
  const clearRestore = useRestore((state) => state.clear);

  function startNew() {
    clearRestore();
    if (route === "image") {
      resetImage();
      setImageFresh((value) => value + 1);
      return;
    }
    if (route === "video") {
      resetVideo();
      setVideoFresh((value) => value + 1);
      return;
    }
    if (route === "audio") {
      resetAudio();
      setAudioFresh((value) => value + 1);
      return;
    }
    reset();
    setFresh((value) => value + 1);
    setRoute("text");
  }

  const startNewRef = useRef(startNew);
  startNewRef.current = startNew;

  const [visited, setVisited] = useState<Record<Route, boolean>>({
    text: true,
    image: false,
    video: false,
    audio: false,
    workflow: false,
    history: false,
    analytics: false,
    developer: false,
    models: false,
    settings: false,
  });

  useEffect(() => {
    setVisited((prev) => (prev[route] ? prev : { ...prev, [route]: true }));
  }, [route]);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && !event.altKey) {
        const key = event.key.toLowerCase();
        if (key === "k") {
          event.preventDefault();
          setSwitcher((prev) => !prev);
          return;
        }
        if (key === "n") {
          event.preventDefault();
          startNewRef.current();
          return;
        }
        if (key === "1") {
          event.preventDefault();
          setRoute("text");
          return;
        }
        if (key === "2") {
          event.preventDefault();
          setRoute("image");
          return;
        }
        if (key === "3") {
          event.preventDefault();
          setRoute("video");
          return;
        }
        if (key === "4") {
          event.preventDefault();
          setRoute("audio");
          return;
        }
        if (key === "5") {
          event.preventDefault();
          setRoute("workflow");
          return;
        }
        if (key === "6") {
          event.preventDefault();
          setRoute("history");
          return;
        }
        if (key === "7") {
          event.preventDefault();
          setRoute("analytics");
          return;
        }
        if (key === "8") {
          event.preventDefault();
          setRoute("developer");
          return;
        }
        if (key === "9") {
          event.preventDefault();
          setRoute("models");
          return;
        }
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    // 首次启动且未跳过向导时，检查是否无已配置 Provider
    const dismissed = localStorage.getItem("modeldeck_setup_wizard_dismissed");
    if (!dismissed) {
      void api
        .providers()
        .then((list) => {
          if (list.length === 0) {
            setShowWizard(true);
          }
        })
        .catch(() => {});
    }

    function onOpenWizard() {
      setShowWizard(true);
    }
    window.addEventListener("open-setup-wizard", onOpenWizard);
    return () => window.removeEventListener("open-setup-wizard", onOpenWizard);
  }, []);

  return (
    <div className="shell">
      <aside className="nav" aria-label="主导航">
        <div className="brand">
          <img
            src="/icon.png"
            alt="ModelDeck"
            style={{
              width: 28,
              height: 28,
              borderRadius: 6,
              objectFit: "cover",
            }}
          />
          <div>
            <strong>ModelDeck</strong>
            <em>模型工作台</em>
          </div>
        </div>
        <div style={{ display: "flex", gap: "6px", margin: "0 0 10px 0" }}>
          <button
            type="button"
            className="nav-new"
            onClick={startNew}
            title="新建工作区草稿 (Ctrl+N)"
            style={{ flex: 1, margin: 0 }}
          >
            新建草稿
          </button>
          <button
            type="button"
            title="打开命令面板 (Ctrl+K)"
            onClick={() => setSwitcher(true)}
            style={{
              padding: "0 10px",
              width: "auto",
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              background: "var(--raised)",
              border: "1px solid var(--line)",
              borderRadius: "var(--radius-md)",
              color: "var(--ink-secondary)",
              margin: 0,
              fontSize: "13px",
              boxShadow: "var(--shadow-sm)",
            }}
          >
            Ctrl K
          </button>
        </div>
        <button
          type="button"
          className="nav-item"
          aria-current={route === "text" ? "page" : undefined}
          onClick={() => setRoute("text")}
        >
          <span style={{ flex: 1 }}>文本</span>
          <kbd style={{ fontSize: "10px", opacity: 0.6 }}>^1</kbd>
        </button>
        <button
          type="button"
          className="nav-item"
          aria-current={route === "image" ? "page" : undefined}
          onClick={() => setRoute("image")}
        >
          <span style={{ flex: 1 }}>图像</span>
          <kbd style={{ fontSize: "10px", opacity: 0.6 }}>^2</kbd>
        </button>
        <button
          type="button"
          className="nav-item"
          aria-current={route === "video" ? "page" : undefined}
          onClick={() => setRoute("video")}
        >
          <span style={{ flex: 1 }}>视频</span>
          <kbd style={{ fontSize: "10px", opacity: 0.6 }}>^3</kbd>
        </button>
        <button
          type="button"
          className="nav-item"
          aria-current={route === "audio" ? "page" : undefined}
          onClick={() => setRoute("audio")}
        >
          <span style={{ flex: 1 }}>音频</span>
          <kbd style={{ fontSize: "10px", opacity: 0.6 }}>^4</kbd>
        </button>
        <button
          type="button"
          className="nav-item"
          aria-current={route === "workflow" ? "page" : undefined}
          onClick={() => setRoute("workflow")}
        >
          <span style={{ flex: 1 }}>工作流</span>
          <kbd style={{ fontSize: "10px", opacity: 0.6 }}>^5</kbd>
        </button>
        <button
          type="button"
          className="nav-item"
          aria-current={route === "history" ? "page" : undefined}
          onClick={() => setRoute("history")}
        >
          <span style={{ flex: 1 }}>历史</span>
          <kbd style={{ fontSize: "10px", opacity: 0.6 }}>^6</kbd>
        </button>
        <button
          type="button"
          className="nav-item"
          aria-current={route === "analytics" ? "page" : undefined}
          onClick={() => setRoute("analytics")}
        >
          <span style={{ flex: 1 }}>用量</span>
          <kbd style={{ fontSize: "10px", opacity: 0.6 }}>^7</kbd>
        </button>
        <button
          type="button"
          className="nav-item"
          aria-current={route === "developer" ? "page" : undefined}
          onClick={() => setRoute("developer")}
        >
          <span style={{ flex: 1 }}>调试器</span>
          <kbd style={{ fontSize: "10px", opacity: 0.6 }}>^8</kbd>
        </button>
        <div className="nav-spacer" />
        <button
          type="button"
          className="nav-item"
          aria-current={route === "models" ? "page" : undefined}
          onClick={() => setRoute("models")}
        >
          <span style={{ flex: 1 }}>模型</span>
          <kbd style={{ fontSize: "10px", opacity: 0.6 }}>^9</kbd>
        </button>
        <button
          type="button"
          className="nav-item"
          aria-current={route === "settings" ? "page" : undefined}
          onClick={() => setRoute("settings")}
        >
          <span>设置</span>
        </button>
        <button
          type="button"
          className="nav-item"
          onClick={() => setShowWizard(true)}
          title="运行首次使用配置向导"
        >
          <span>🚀 新手向导</span>
        </button>
      </aside>
      <main className="main">
        <Suspense
          fallback={
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                height: "100%",
                color: "var(--muted, #8b949e)",
                fontSize: "14px",
              }}
            >
              正在加载模块...
            </div>
          }
        >
          <div style={{ display: route === "text" ? "contents" : "none" }}>
            <ErrorBoundary moduleName="文本 (Text)">
              <TextView fresh={fresh} onOpenModels={() => setRoute("models")} />
            </ErrorBoundary>
          </div>
          {visited.image ? (
            <div style={{ display: route === "image" ? "contents" : "none" }}>
              <ErrorBoundary moduleName="图像 (Image)">
                <ImageView
                  fresh={imageFresh}
                  onOpenModels={() => setRoute("models")}
                />
              </ErrorBoundary>
            </div>
          ) : null}
          {visited.video ? (
            <div style={{ display: route === "video" ? "contents" : "none" }}>
              <ErrorBoundary moduleName="视频 (Video)">
                <VideoView
                  fresh={videoFresh}
                  onOpenModels={() => setRoute("models")}
                />
              </ErrorBoundary>
            </div>
          ) : null}
          {visited.audio ? (
            <div style={{ display: route === "audio" ? "contents" : "none" }}>
              <ErrorBoundary moduleName="音频 (Audio)">
                <AudioView
                  fresh={audioFresh}
                  onOpenModels={() => setRoute("models")}
                />
              </ErrorBoundary>
            </div>
          ) : null}
          {visited.workflow ? (
            <div
              style={{ display: route === "workflow" ? "contents" : "none" }}
            >
              <ErrorBoundary moduleName="工作流 (Workflow)">
                <PipelineView onOpenModels={() => setRoute("models")} />
              </ErrorBoundary>
            </div>
          ) : null}
          {visited.history ? (
            <div style={{ display: route === "history" ? "contents" : "none" }}>
              <ErrorBoundary moduleName="历史记录 (History)">
                <HistoryView
                  onOpenText={() => setRoute("text")}
                  onOpenImage={() => setRoute("image")}
                  onOpenVideo={() => setRoute("video")}
                  onOpenAudio={() => setRoute("audio")}
                />
              </ErrorBoundary>
            </div>
          ) : null}
          {visited.analytics ? (
            <div
              style={{ display: route === "analytics" ? "contents" : "none" }}
            >
              <ErrorBoundary moduleName="用量分析 (Analytics)">
                <AnalyticsView />
              </ErrorBoundary>
            </div>
          ) : null}
          {visited.developer ? (
            <div
              style={{ display: route === "developer" ? "contents" : "none" }}
            >
              <ErrorBoundary moduleName="调试器 (Developer)">
                <DeveloperView />
              </ErrorBoundary>
            </div>
          ) : null}
          {visited.models ? (
            <div style={{ display: route === "models" ? "contents" : "none" }}>
              <ErrorBoundary moduleName="模型管理 (Models)">
                <ModelsView onOpenSettings={() => setRoute("settings")} />
              </ErrorBoundary>
            </div>
          ) : null}
          {visited.settings ? (
            <div
              style={{ display: route === "settings" ? "contents" : "none" }}
            >
              <ErrorBoundary moduleName="设置 (Settings)">
                <SettingsView />
              </ErrorBoundary>
            </div>
          ) : null}
        </Suspense>
      </main>
      {switcher ? (
        <CommandPalette
          onClose={() => setSwitcher(false)}
          onNavigate={(target) => setRoute(target as Route)}
          onStartNew={startNew}
        />
      ) : null}
      {showWizard ? (
        <SetupWizardModal
          onClose={() => setShowWizard(false)}
          onFinishAndNavigate={(targetRoute) => {
            setShowWizard(false);
            setRoute(targetRoute as Route);
          }}
        />
      ) : null}
    </div>
  );
}
