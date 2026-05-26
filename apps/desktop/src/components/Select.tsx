import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";

export interface SelectOption {
  value: string;
  label: string;
  hint?: string;
  disabled?: boolean;
}

interface SelectProps {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  buttonClassName?: string;
  ariaLabel?: string;
  /** Show a search input at the top of the popover that filters options by
   *  label/hint substring. Up/Down arrow keys are suppressed while the search
   *  input has focus so the user can navigate the text caret without
   *  accidentally cycling the active option. Defaults to false (existing
   *  call-sites keep their plain-dropdown behavior). */
  searchable?: boolean;
}

export function Select({
  value,
  options,
  onChange,
  placeholder = "Select…",
  disabled,
  className,
  buttonClassName,
  ariaLabel,
  searchable = false,
}: SelectProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const [rect, setRect] = useState<{
    left: number;
    top: number;
    width: number;
    placement: "below" | "above";
    maxHeight: number;
  } | null>(null);

  const current = useMemo(
    () => options.find((o) => o.value === value),
    [options, value],
  );

  // Filtered list — case-insensitive match against label OR hint. Empty
  // query returns the unfiltered options. Always falls back to the original
  // list if every option got filtered out (so the user can see "no matches"
  // and still see the dropdown didn't collapse).
  const filteredOptions = useMemo(() => {
    if (!searchable) return options;
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter(
      (o) =>
        o.label.toLowerCase().includes(q) ||
        (o.hint?.toLowerCase().includes(q) ?? false),
    );
  }, [options, query, searchable]);

  const computeRect = useCallback(() => {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    const viewportH = window.innerHeight;
    const margin = 8;
    const spaceBelow = viewportH - r.bottom - margin;
    const spaceAbove = r.top - margin;
    const desiredMax = 320;
    let placement: "below" | "above" = "below";
    let maxHeight = Math.min(desiredMax, spaceBelow);
    if (spaceBelow < 160 && spaceAbove > spaceBelow) {
      placement = "above";
      maxHeight = Math.min(desiredMax, spaceAbove);
    }
    maxHeight = Math.max(maxHeight, 120);
    setRect({
      left: r.left,
      top: placement === "below" ? r.bottom + 4 : r.top - 4,
      width: r.width,
      placement,
      maxHeight,
    });
  }, []);

  useLayoutEffect(() => {
    if (!open) return;
    computeRect();
    const handle = () => computeRect();
    window.addEventListener("scroll", handle, true);
    window.addEventListener("resize", handle);
    return () => {
      window.removeEventListener("scroll", handle, true);
      window.removeEventListener("resize", handle);
    };
  }, [open, computeRect]);

  // Clear the search query each time the popover closes so the next open
  // starts fresh.
  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onDocPointer = (e: PointerEvent) => {
      const target = e.target as Node | null;
      if (
        triggerRef.current?.contains(target) ||
        listRef.current?.contains(target)
      ) {
        return;
      }
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        return;
      }
      // While the search input has focus, leave up/down alone so the user
      // can navigate within their typed text and not have the dropdown
      // cycle the active option underneath them. (Enter / Escape still
      // work for commit + dismiss via the input's own onKeyDown.)
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        if (
          searchable &&
          searchInputRef.current &&
          document.activeElement === searchInputRef.current
        ) {
          return;
        }
        e.preventDefault();
        if (filteredOptions.length === 0) return;
        const enabled = filteredOptions
          .map((o, i) => ({ o, i }))
          .filter(({ o }) => !o.disabled);
        if (enabled.length === 0) return;
        const currentIdx = enabled.findIndex(({ o }) => o.value === value);
        const step = e.key === "ArrowDown" ? 1 : -1;
        const nextPos =
          currentIdx < 0
            ? step > 0
              ? 0
              : enabled.length - 1
            : (currentIdx + step + enabled.length) % enabled.length;
        const next = enabled[nextPos];
        if (next) onChange(next.o.value);
        return;
      }
      if (e.key === "Enter" || e.key === " ") {
        // Suppress " " (space) and Enter only when not currently typing in
        // the search input — those keys should reach the input untouched.
        if (
          searchable &&
          searchInputRef.current &&
          document.activeElement === searchInputRef.current
        ) {
          if (e.key === "Enter") {
            // Treat Enter in the search input as "commit the first match
            // and close". Useful for fast keyboard navigation.
            const firstEnabled = filteredOptions.find((o) => !o.disabled);
            if (firstEnabled) {
              onChange(firstEnabled.value);
              setOpen(false);
              e.preventDefault();
            }
          }
          return;
        }
        e.preventDefault();
        setOpen(false);
      }
    };
    document.addEventListener("pointerdown", onDocPointer, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDocPointer, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, filteredOptions, value, onChange, searchable]);

  // Focus the search input shortly after the popover opens. The small
  // timeout lets the portal node mount before we steal focus from the
  // trigger button.
  useLayoutEffect(() => {
    if (!open || !searchable) return;
    const id = window.setTimeout(() => {
      searchInputRef.current?.focus();
    }, 30);
    return () => window.clearTimeout(id);
  }, [open, searchable]);

  // Always keep the currently-active option in view — fires when the
  // popover opens, when the active value changes (e.g. an animation
  // started playing externally), or when the user types into the search
  // input and the filtered list reshapes around the selection.
  useLayoutEffect(() => {
    if (!open) return;
    const list = listRef.current;
    if (!list) return;
    const selected = list.querySelector<HTMLLIElement>(
      ".select-option.is-selected",
    );
    if (selected) {
      selected.scrollIntoView({ block: "nearest" });
    }
  }, [open, value, filteredOptions]);

  const buttonLabel = current?.label ?? placeholder;
  const buttonHint = current?.hint;

  return (
    <div className={`select-root ${className ?? ""}`}>
      <button
        ref={triggerRef}
        type="button"
        className={`select-trigger ${buttonClassName ?? ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        disabled={disabled}
        onClick={() => {
          if (disabled) return;
          setOpen((v) => !v);
        }}
      >
        <span className={`select-trigger-label${current ? "" : " dim"}`}>
          {buttonLabel}
          {buttonHint && <span className="select-trigger-hint dim small"> {buttonHint}</span>}
        </span>
        <span className="select-trigger-caret" aria-hidden>
          {open ? "▴" : "▾"}
        </span>
      </button>
      {open &&
        rect &&
        createPortal(
          <div
            className="select-popover-wrap"
            style={{
              position: "fixed",
              left: rect.left,
              width: rect.width,
              ...(rect.placement === "below"
                ? { top: rect.top }
                : { bottom: window.innerHeight - rect.top }),
            }}
          >
            {searchable && (
              <div className="select-search">
                <input
                  ref={searchInputRef}
                  type="search"
                  className="select-search-input"
                  placeholder="Filter…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  aria-label="Filter options"
                  onKeyDown={(e) => {
                    // Hard-stop arrow keys at the input so the document
                    // listener above can't cycle the active option even
                    // if focus is briefly transitioning. Enter is handled
                    // by the document listener (commits the first match).
                    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
                      e.stopPropagation();
                    }
                    if (e.key === "Escape" && query) {
                      // First Escape clears the filter; second closes the
                      // popover via the document listener.
                      setQuery("");
                      e.stopPropagation();
                    }
                  }}
                />
                {query && (
                  <button
                    type="button"
                    className="select-search-clear"
                    onClick={() => {
                      setQuery("");
                      searchInputRef.current?.focus();
                    }}
                    aria-label="Clear filter"
                    title="Clear filter"
                  >
                    ✕
                  </button>
                )}
              </div>
            )}
            <ul
              ref={listRef}
              className="select-popover"
              role="listbox"
              style={{ maxHeight: rect.maxHeight }}
            >
              {filteredOptions.length === 0 && (
                <li className="select-empty dim small">
                  {searchable && query ? "No matches" : "No options"}
                </li>
              )}
              {filteredOptions.map((opt) => {
                const selected = opt.value === value;
                return (
                  <li
                    key={opt.value}
                    role="option"
                    aria-selected={selected}
                    className={`select-option${selected ? " is-selected" : ""}${opt.disabled ? " is-disabled" : ""}`}
                    onClick={() => {
                      if (opt.disabled) return;
                      onChange(opt.value);
                      setOpen(false);
                    }}
                  >
                    <span className="select-option-label">{opt.label}</span>
                    {opt.hint && (
                      <span className="select-option-hint dim small">{opt.hint}</span>
                    )}
                  </li>
                );
              })}
            </ul>
          </div>,
          document.body,
        )}
    </div>
  );
}
