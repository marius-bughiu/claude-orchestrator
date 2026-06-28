/// The app mark: an orchestrator hub with three orbiting agent nodes.
export function Logo({ size = 24 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 48 48" fill="none" xmlns="http://www.w3.org/2000/svg" aria-label="Claude Orchestrator">
      <defs>
        <linearGradient id="logo-g" x1="0" y1="0" x2="48" y2="48" gradientUnits="userSpaceOnUse">
          <stop stopColor="#818cf8" />
          <stop offset="1" stopColor="#6366f1" />
        </linearGradient>
      </defs>
      {/* connecting spokes */}
      <g stroke="url(#logo-g)" strokeWidth="2.4" strokeLinecap="round" opacity="0.55">
        <line x1="24" y1="24" x2="24" y2="9" />
        <line x1="24" y1="24" x2="37" y2="32" />
        <line x1="24" y1="24" x2="11" y2="32" />
      </g>
      {/* hub */}
      <circle cx="24" cy="24" r="5.4" fill="url(#logo-g)" />
      {/* orbiting nodes */}
      <circle cx="24" cy="9" r="4.2" fill="#a5b4fc" />
      <circle cx="37" cy="32" r="4.2" fill="#34d399" />
      <circle cx="11" cy="32" r="4.2" fill="#38bdf8" />
    </svg>
  );
}
