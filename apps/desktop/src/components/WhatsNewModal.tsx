import {
  useMemo,
  type AnchorHTMLAttributes,
  type MouseEvent,
} from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { Modal } from "./Modal";
import { APP_VERSION, openExternal } from "../version";

// RELEASE_NOTES.md lives at the repo root. Bundled via Vite's ?raw query so
// the modal renders the exact file shipped with this build — no fetch, no
// network, no file-system permission required at runtime.
//   apps/desktop/src/components/  ->  apps/desktop/src/  ->  apps/desktop/
//   ->  apps/  ->  <repo root>/RELEASE_NOTES.md
import releaseNotesRaw from "../../../../RELEASE_NOTES.md?raw";

interface WhatsNewModalProps {
  open: boolean;
  onClose: () => void;
}

export function WhatsNewModal({ open, onClose }: WhatsNewModalProps) {
  const markdownComponents = useMemo<Components>(
    () => ({
      a: ({
        href,
        children,
        ...rest
      }: AnchorHTMLAttributes<HTMLAnchorElement>) => {
        const isExternal = !!href && /^https?:\/\//i.test(href);
        if (!isExternal) {
          return (
            <a href={href} {...rest}>
              {children}
            </a>
          );
        }
        return (
          <a
            href={href}
            onClick={(e: MouseEvent<HTMLAnchorElement>) => {
              e.preventDefault();
              if (href) void openExternal(href);
            }}
            {...rest}
          >
            {children}
          </a>
        );
      },
    }),
    [],
  );

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={`What's new in v${APP_VERSION}`}
      subtitle="Release notes for this version"
      size="lg"
      bodyClassName="whats-new-modal-body"
      footer={
        <button type="button" className="btn btn-primary" onClick={onClose}>
          Got it
        </button>
      }
    >
      <div className="whats-new-markdown">
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={markdownComponents}
        >
          {releaseNotesRaw}
        </ReactMarkdown>
      </div>
    </Modal>
  );
}
