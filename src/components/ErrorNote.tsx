import { useState } from "react";
import type { AppError } from "../types";

const TITLES: Record<string, string> = {
  AuthenticationError: "认证失败",
  NetworkError: "网络错误",
  TimeoutError: "请求超时",
  RateLimitError: "请求频率超过限制",
  InvalidRequestError: "请求无效",
  UnsupportedCapabilityError: "能力不支持",
  ProviderError: "Provider 错误",
  ValidationError: "校验未通过",
  StorageError: "本地存储错误",
  NotFoundError: "找不到记录",
  cancelled: "已取消",
  task_failed: "生成失败",
};

export function ErrorNote({ error }: { error: AppError }) {
  const [open, setOpen] = useState(false);
  return (
    <div
      className={
        error.code === "cancelled" ? "error-note cancelled-note" : "error-note"
      }
      role={error.code === "cancelled" ? "status" : "alert"}
    >
      <strong>
        {error.status ? `${error.status} · ` : ""}
        {TITLES[error.code] ?? error.code}
      </strong>
      <p>{error.message}</p>
      {error.provider ? <p>Provider：{error.provider}</p> : null}
      {error.model ? <p>Model：{error.model}</p> : null}
      {error.raw ? (
        <button
          type="button"
          className="text-button"
          onClick={() => setOpen((value) => !value)}
        >
          {open ? "隐藏原始错误" : "查看原始错误"}
        </button>
      ) : null}
      {open ? <pre>{JSON.stringify(error.raw, null, 2)}</pre> : null}
    </div>
  );
}
