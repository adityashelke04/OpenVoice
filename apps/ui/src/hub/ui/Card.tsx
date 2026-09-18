import type { ElementType, ReactNode } from "react";

/** Reference `.card.glass`: title row (icon + display-face title) with an optional
 *  muted right slot, then the body. `as` lets a card be a `<section>` when it is a
 *  page region rather than a self-contained item. */
export function Card({ title, icon, right, className, as: Tag = "article", children }: {
  title: ReactNode; icon?: ReactNode; right?: ReactNode; className?: string; as?: ElementType; children?: ReactNode;
}) {
  return (
    <Tag className={["card glass", className].filter(Boolean).join(" ")}>
      <div className="card-head">
        <div className="card-title">{icon}{title}</div>
        {right != null && <span className="right cap">{right}</span>}
      </div>
      {children}
    </Tag>
  );
}
