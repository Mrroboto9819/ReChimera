import { forwardRef, useEffect, useRef, useState, type FocusEvent } from "react";
import { Search, X } from "lucide-react";

interface SearchFieldProps {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
  ariaLabel?: string;
  className?: string;
  autoFocus?: boolean;
  disabled?: boolean;
  /** Slash key globally focuses this field. Off by default. */
  hotkey?: "/" | null;
}

export const SearchField = forwardRef<HTMLInputElement, SearchFieldProps>(
  function SearchField(
    {
      value,
      onChange,
      placeholder = "Search…",
      ariaLabel,
      className,
      autoFocus,
      disabled,
      hotkey = null,
    },
    refForward,
  ) {
    const innerRef = useRef<HTMLInputElement | null>(null);
    const [focused, setFocused] = useState(false);

    const setRefs = (node: HTMLInputElement | null) => {
      innerRef.current = node;
      if (typeof refForward === "function") refForward(node);
      else if (refForward) refForward.current = node;
    };

    useEffect(() => {
      if (!hotkey) return;
      const handler = (e: KeyboardEvent) => {
        if (e.key !== hotkey) return;
        const target = e.target as HTMLElement | null;
        if (
          target &&
          (target.tagName === "INPUT" ||
            target.tagName === "TEXTAREA" ||
            target.isContentEditable)
        ) {
          return;
        }
        e.preventDefault();
        innerRef.current?.focus();
      };
      window.addEventListener("keydown", handler);
      return () => window.removeEventListener("keydown", handler);
    }, [hotkey]);

    return (
      <div
        className={`search-field${focused ? " is-focused" : ""}${
          value ? " has-value" : ""
        }${className ? ` ${className}` : ""}`}
      >
        <Search size={14} className="search-field-icon" aria-hidden />
        <input
          ref={setRefs}
          type="text"
          className="search-field-input"
          placeholder={placeholder}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onFocus={() => setFocused(true)}
          onBlur={(e: FocusEvent<HTMLInputElement>) => {
            setFocused(false);
            e.currentTarget.scrollLeft = 0;
          }}
          spellCheck={false}
          aria-label={ariaLabel ?? placeholder}
          autoFocus={autoFocus}
          disabled={disabled}
        />
        {value && (
          <button
            type="button"
            className="search-field-clear"
            onClick={() => {
              onChange("");
              innerRef.current?.focus();
            }}
            aria-label="Clear search"
            tabIndex={-1}
          >
            <X size={12} strokeWidth={2} />
          </button>
        )}
      </div>
    );
  },
);
