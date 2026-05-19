import { useEffect, useRef, useState } from "react";
import { ChevronDown, Download } from "lucide-react";
import { Button } from "../ui";
import {
  setLastExportFormat,
  useAppDispatch,
  useAppSelector,
  type ExportFormat,
} from "../store";

interface ExportFormatPickerProps {
  onExport: (format: ExportFormat) => void;
  disabled?: boolean;
  loading?: boolean;
  busyLabel?: string;
  label?: (format: ExportFormat) => string;
  placement?: "above" | "below";
}

const DEFAULT_LABEL = (format: ExportFormat) =>
  format === "fbx" ? "Export .fbx" : "Export .glb";

const FORMAT_OPTIONS: ReadonlyArray<{
  id: ExportFormat;
  title: string;
  hint: string;
  badge?: string;
}> = [
  {
    id: "glb",
    title: "glTF binary (.glb)",
    hint: "Universal. Animations, materials & textures embedded.",
  },
  {
    id: "fbx",
    title: "Autodesk FBX (.fbx)",
    hint: "Binary 7.4 — mesh, materials, textures, skinning & animation. Imports into Blender, Unreal, Maya.",
    badge: "beta",
  },
];

export function ExportFormatPicker({
  onExport,
  disabled,
  loading,
  busyLabel,
  label = DEFAULT_LABEL,
  placement = "above",
}: ExportFormatPickerProps) {
  const format =
    useAppSelector((s) => s.settings.lastExportFormat) ?? "glb";
  const dispatch = useAppDispatch();
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!containerRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  const choose = (next: ExportFormat) => {
    dispatch(setLastExportFormat(next));
    setOpen(false);
  };

  return (
    <div ref={containerRef} className="export-format-picker">
      <Button
        variant="primary"
        icon={Download}
        onClick={() => onExport(format)}
        disabled={disabled}
        loading={loading}
        className="export-format-main"
      >
        {loading ? (busyLabel ?? "Exporting…") : label(format)}
      </Button>
      <button
        type="button"
        className="btn btn-primary export-format-chevron"
        onClick={() => setOpen((o) => !o)}
        disabled={disabled || loading}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Choose export format"
      >
        <ChevronDown size={14} strokeWidth={2} />
      </button>
      {open && (
        <ul
          className={`export-format-menu export-format-menu-${placement}`}
          role="menu"
        >
          {FORMAT_OPTIONS.map((opt) => (
            <li key={opt.id}>
              <button
                type="button"
                role="menuitemradio"
                aria-checked={format === opt.id}
                className={`export-format-option${format === opt.id ? " is-active" : ""}`}
                onClick={() => choose(opt.id)}
              >
                <span className="export-format-option-title">
                  <strong>{opt.title}</strong>
                  {opt.badge && (
                    <span className="export-format-option-badge">
                      {opt.badge}
                    </span>
                  )}
                </span>
                <span className="dim small">{opt.hint}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
