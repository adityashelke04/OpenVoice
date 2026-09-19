/** The toast stack, bottom right (see toast.ts for why notices are toasts).
 *
 *  Each toast is its own polite live region, so a screen reader reads the
 *  message once when it lands and not again when a second one stacks on top. */
import { Info, Warning, WarningOctagon, X } from "@phosphor-icons/react";
import { dismissToast, useToasts, type ToastTone } from "./toast";

const ICON: Record<ToastTone, typeof Info> = { info: Info, warn: Warning, danger: WarningOctagon };

export function Toasts() {
  const toasts = useToasts();
  if (toasts.length === 0) return null;
  return (
    <div className="toasts">
      {toasts.map(({ id, tone, message }) => {
        const Icon = ICON[tone];
        return (
          <div key={id} className={`toast glass ${tone}`} role="status">
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
