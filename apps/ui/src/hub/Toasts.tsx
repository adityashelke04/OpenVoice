/** The toast stack, bottom right (see toast.ts for why notices are toasts).
 *
 *  One polite live region that is always in the document, with toasts inserted
 *  into it. A region created together with its first message is often missed
 *  by screen readers (they only watch regions that already exist), so the
 *  container never unmounts; empty, it takes no space and catches no clicks. */
import { Info, Warning, WarningOctagon, X } from "@phosphor-icons/react";
import { dismissToast, useToasts, type ToastTone } from "./toast";

const ICON: Record<ToastTone, typeof Info> = { info: Info, warn: Warning, danger: WarningOctagon };

export function Toasts() {
  const toasts = useToasts();
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map(({ id, tone, message }) => {
        const Icon = ICON[tone];
        return (
          <div key={id} className={`toast glass ${tone}`}>
            <Icon className="tone" size="1em" weight="bold" aria-hidden="true" />
            <span className="msg">{message}</span>
            <button type="button" className="dismiss" aria-label="Dismiss" title="Dismiss" onClick={() => dismissToast(id)}>
              <X size="1em" weight="bold" aria-hidden="true" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
