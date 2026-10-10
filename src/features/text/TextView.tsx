import { useEffect, useState } from "react";
import { useRestore } from "../../store";
import { ChatView } from "./ChatView";
import { CompareView } from "./CompareView";
import { GenerateView } from "./GenerateView";

export function TextView({
  onOpenModels,
  fresh,
}: {
  onOpenModels: () => void;
  fresh: number;
}) {
  const restore = useRestore((state) => state.request);
  const [mode, setMode] = useState<"generate" | "chat" | "compare">(
    restore?.kind === "chat" ? "chat" : "generate",
  );
  useEffect(() => {
    if (!restore || (restore.kind !== "chat" && restore.kind !== "generate")) {
      return;
    }
    setMode(restore.kind);
  }, [restore]);

  if (mode === "compare") {
    return (
      <CompareView
        onOpenModels={onOpenModels}
        onShowGenerate={() => setMode("generate")}
        onShowChat={() => setMode("chat")}
      />
    );
  }

  if (mode === "chat") {
    return (
      <ChatView
        fresh={fresh}
        onOpenModels={onOpenModels}
        onShowGenerate={() => setMode("generate")}
        onShowCompare={() => setMode("compare")}
      />
    );
  }
  return (
    <GenerateView
      onOpenModels={onOpenModels}
      onShowChat={() => setMode("chat")}
      onShowCompare={() => setMode("compare")}
    />
  );
}
