import { fileName } from "../lib/schema";

export function FileList({
  label,
  paths,
  multiple,
  onChoose,
  onChange,
}: {
  label: string;
  paths: string[];
  multiple: boolean;
  onChoose: () => Promise<string[]>;
  onChange: (paths: string[]) => void;
}) {
  return (
    <div className="file-list">
      <div className="file-list-head">
        <span>{label}</span>
        <button
          type="button"
          onClick={() => {
            void onChoose().then((picked) => {
              if (picked.length === 0) {
                return;
              }
              onChange(
                multiple ? unique([...paths, ...picked]) : picked.slice(0, 1),
              );
            });
          }}
        >
          选择文件
        </button>
      </div>
      {paths.length === 0 ? <p className="muted">未选择</p> : null}
      <ul>
        {paths.map((path) => (
          <li key={path}>
            <span title={path}>{fileName(path)}</span>
            <button
              type="button"
              onClick={() => onChange(paths.filter((item) => item !== path))}
            >
              移除
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function unique(paths: string[]): string[] {
  return [...new Set(paths)];
}
