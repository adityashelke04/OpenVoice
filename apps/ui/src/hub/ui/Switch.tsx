/** Reference `.switch`: a real `<button role="switch">`, so Space/Enter toggle it
 *  for free and screen readers announce on/off. `label` is the accessible name; the
 *  visible label lives in the surrounding SettingRow. */
export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={checked ? "switch on" : "switch"}
      onClick={() => onChange(!checked)}
    />
  );
}
