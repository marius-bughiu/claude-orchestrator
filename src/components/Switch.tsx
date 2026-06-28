import clsx from "clsx";

/// An accessible on/off switch.
export function Switch({
  checked,
  onChange,
  label,
  size = "md",
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label?: string;
  size?: "sm" | "md";
}) {
  const dims = size === "sm" ? { w: "w-8", h: "h-4.5", knob: "h-3.5 w-3.5", on: "translate-x-3.5" } : { w: "w-10", h: "h-5.5", knob: "h-4.5 w-4.5", on: "translate-x-[18px]" };
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!checked);
      }}
      className={clsx(
        "relative inline-flex shrink-0 items-center rounded-full p-0.5 transition-colors",
        dims.w,
        size === "sm" ? "h-[18px]" : "h-[22px]",
        checked ? "bg-indigo-600" : "bg-neutral-700",
      )}
    >
      <span
        className={clsx(
          "inline-block transform rounded-full bg-white shadow transition-transform",
          dims.knob,
          checked ? dims.on : "translate-x-0",
        )}
      />
    </button>
  );
}
