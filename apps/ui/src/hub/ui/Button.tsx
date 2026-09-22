import type { ButtonHTMLAttributes, ReactNode } from "react";

/** `.btn` from reference §4. `variant="warn"` maps to the reference's `.warnp`
 *  (amber fill), which is reserved for "not pasted" recovery actions. `kbd` is the
 *  quiet key hint the reference shows inside primary actions ("Ctrl C").
 *  `type` defaults to "button" so a Button inside a form never submits it. */
export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "default" | "primary" | "warn";
  size?: "sm";
  icon?: ReactNode;
  kbd?: string;
};

export function Button({ variant = "default", size, icon, kbd, className, type = "button", children, ...rest }: ButtonProps) {
  const cls = ["btn", variant === "primary" && "primary", variant === "warn" && "warnp", size, className].filter(Boolean).join(" ");
  return (
    <button type={type} className={cls} {...rest}>
      {icon}
      {children}
      {kbd && <span className="k">{kbd}</span>}
    </button>
  );
}
