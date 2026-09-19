import type { CSSProperties, ElementType, ReactNode } from "react";

/** Reference `.card.glass`: title row (icon + display-face title) with an optional
 *  muted right slot, then the body. `as` lets a card be a `<section>` when it is a
 *  page region rather than a self-contained item.
 *
 *  Two further head slots, for cards like the Dictionary's "Your corrections":
 *  `meta` is a muted caption sitting right after the title (a count), and
 *  `actions` is a right-aligned control group (a search field). `right` stays a
 *  caption; a field inside the caption's 12px mute span would inherit both. */
export function Card({ title, icon, right, meta, actions, className, style, as: Tag = "article", children }: {
  title: ReactNode; icon?: ReactNode; right?: ReactNode; meta?: ReactNode; actions?: ReactNode;
  className?: string; style?: CSSProperties; as?: ElementType; children?: ReactNode;
}) {
  return (
    <Tag className={["card glass", className].filter(Boolean).join(" ")} style={style}>
      <div className="card-head">
        <div className="card-title">{icon}{title}</div>
        {meta != null && <span className="cap">{meta}</span>}
        {right != null && <span className="right cap">{right}</span>}
        {actions != null && <div className="right">{actions}</div>}
      </div>
      {children}
    </Tag>
  );
}
