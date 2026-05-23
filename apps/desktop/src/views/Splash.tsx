import { useEffect, useLayoutEffect, useRef } from "react";
import gsap from "gsap";
import iconUrl from "../../icon.png?url";
import { APP_BRAND_NAME } from "../version";

interface SplashProps {
  
  visible: boolean;
  
  onExit: () => void;
}











export function Splash({ visible, onExit }: SplashProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const logoRef = useRef<HTMLDivElement>(null);
  const nameRef = useRef<HTMLHeadingElement>(null);
  const loaderRef = useRef<HTMLDivElement>(null);

  
  
  
  
  
  
  
  
  
  
  
  
  useLayoutEffect(() => {
    const tl = gsap.timeline();
    tl.to(rootRef.current, {
      opacity: 1,
      duration: 0.25,
      ease: "power2.out",
    })
      .to(
        logoRef.current,
        {
          opacity: 1,
          scale: 1,
          y: 0,
          duration: 0.6,
          ease: "back.out(1.4)",
        },
        "-=0.1",
      )
      .to(
        nameRef.current,
        {
          opacity: 1,
          y: 0,
          duration: 0.45,
          ease: "power3.out",
        },
        "-=0.30",
      )
      .to(
        loaderRef.current,
        {
          opacity: 1,
          y: 0,
          duration: 0.35,
          ease: "power2.out",
        },
        "-=0.15",
      );
    return () => {
      tl.kill();
    };
  }, []);

  
  useEffect(() => {
    if (visible) return;
    const root = rootRef.current;
    if (!root) {
      onExit();
      return;
    }
    const tween = gsap.to(root, {
      opacity: 0,
      duration: 0.35,
      ease: "power2.in",
      onComplete: onExit,
    });
    return () => {
      tween.kill();
    };
  }, [visible, onExit]);

  
  
  
  
  
  return (
    <div
      ref={rootRef}
      className="splash"
      role="status"
      aria-live="polite"
      style={{ opacity: 0 }}
    >
      <div className="splash-content">
        <div
          ref={logoRef}
          className="splash-logo"
          style={{ opacity: 0, transform: "scale(0.7) translateY(-8px)" }}
        >
          <img src={iconUrl} alt="" draggable={false} />
        </div>
        <h1
          ref={nameRef}
          className="splash-name"
          style={{ opacity: 0, transform: "translateY(14px)" }}
        >
          {APP_BRAND_NAME}
        </h1>
        <div
          ref={loaderRef}
          className="splash-loader"
          aria-hidden
          style={{ opacity: 0, transform: "translateY(6px)" }}
        >
          <span className="splash-loader-dot" />
          <span className="splash-loader-dot" />
          <span className="splash-loader-dot" />
        </div>
      </div>
    </div>
  );
}
