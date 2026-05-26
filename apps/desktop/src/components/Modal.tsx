import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import gsap from "gsap";
import { useAppSelector, getAppSkin } from "../store";

interface ModalProps {
  
  open: boolean;
  
  onClose?: () => void;
  




  dismissable?: boolean;
  
  title?: ReactNode;

  subtitle?: ReactNode;

  subheader?: ReactNode;

  footer?: ReactNode;

  size?: "sm" | "md" | "lg" | "xl";
  





  bodyClassName?: string;
  children: ReactNode;
}

const SIZE_WIDTH: Record<NonNullable<ModalProps["size"]>, number> = {
  sm: 360,
  md: 480,
  lg: 640,
  
  
  xl: 1100,
};










export function Modal({
  open,
  onClose,
  dismissable = true,
  title,
  subtitle,
  subheader,
  footer,
  size = "md",
  bodyClassName,
  children,
}: ModalProps) {
  const appSkin = useAppSelector((s) => s.settings.appSkin);
  const skinDef = getAppSkin(appSkin);
  const modalClass = skinDef.modalClass;
  const isHud = modalClass === "modal-dialog--hud";
  const backdropRef = useRef<HTMLDivElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  
  
  const tlRef = useRef<gsap.core.Timeline | null>(null);

  
  
  useLayoutEffect(() => {
    const backdrop = backdropRef.current;
    const dialog = dialogRef.current;
    if (!backdrop || !dialog) return;

    tlRef.current?.kill();

    if (open) {
      backdrop.style.display = "flex";
      const tl = gsap.timeline();
      tl.fromTo(
        backdrop,
        { autoAlpha: 0, backdropFilter: "blur(0px)" },
        { autoAlpha: 1, backdropFilter: "blur(8px)", duration: 0.18, ease: "power2.out" },
      ).fromTo(
        dialog,
        { autoAlpha: 0, y: 12, scale: 0.96 },
        { autoAlpha: 1, y: 0, scale: 1, duration: 0.22, ease: "power3.out" },
        "-=0.10",
      );
      tlRef.current = tl;
    } else {
      const tl = gsap.timeline({
        onComplete: () => {
          if (backdropRef.current) backdropRef.current.style.display = "none";
        },
      });
      tl.to(dialog, {
        autoAlpha: 0,
        y: 6,
        scale: 0.98,
        duration: 0.14,
        ease: "power2.in",
      }).to(
        backdrop,
        {
          autoAlpha: 0,
          backdropFilter: "blur(0px)",
          duration: 0.16,
          ease: "power2.in",
        },
        "-=0.06",
      );
      tlRef.current = tl;
    }
  }, [open]);

  
  useEffect(() => {
    if (!open || !dismissable) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose?.();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [open, dismissable, onClose]);

  
  useEffect(() => {
    if (!open) return;
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = prev;
    };
  }, [open]);

  const onBackdropClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (!dismissable) return;
      if (e.target === e.currentTarget) onClose?.();
    },
    [dismissable, onClose],
  );

  const node = (
    <div
      ref={backdropRef}
      className="modal-backdrop"
      onClick={onBackdropClick}
      style={{ display: "none" }}
      role="presentation"
    >
      <div
        ref={dialogRef}
        className={`modal-dialog${modalClass ? ` ${modalClass}` : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? "modal-title" : undefined}
        style={{ width: SIZE_WIDTH[size] }}
      >
        {isHud && (
          <>
            <span className="hud-corner hud-corner--tl" />
            <span className="hud-corner hud-corner--tr" />
            <span className="hud-corner hud-corner--bl" />
            <span className="hud-corner hud-corner--br" />
          </>
        )}
        {(title || dismissable) && (
          <header className="modal-header">
            <div className="modal-header-text">
              {title && (
                <h2 id="modal-title" className="modal-title">
                  {title}
                </h2>
              )}
              {subtitle && <p className="modal-subtitle">{subtitle}</p>}
            </div>
            {dismissable && (
              <button
                type="button"
                className="modal-close"
                onClick={() => onClose?.()}
                aria-label="Close"
              >
                ×
              </button>
            )}
          </header>
        )}
        {subheader && <div className="modal-subheader">{subheader}</div>}
        <div className={`modal-body${bodyClassName ? ` ${bodyClassName}` : ""}`}>
          {children}
        </div>
        {footer && <footer className="modal-footer">{footer}</footer>}
      </div>
    </div>
  );

  return createPortal(node, document.body);
}
