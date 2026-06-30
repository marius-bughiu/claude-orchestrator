import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

// Dark-theme element overrides so agent markdown matches the rest of the UI.
// Note: react-markdown does not render raw HTML, so agent output cannot inject
// markup — keep it that way (no rehype-raw).
const components: Components = {
  p: ({ children }) => <p className="my-1.5 leading-relaxed first:mt-0 last:mb-0">{children}</p>,
  h1: ({ children }) => <h1 className="mb-1.5 mt-3 text-base font-semibold first:mt-0">{children}</h1>,
  h2: ({ children }) => <h2 className="mb-1.5 mt-3 text-sm font-semibold first:mt-0">{children}</h2>,
  h3: ({ children }) => <h3 className="mb-1 mt-2 text-sm font-semibold first:mt-0">{children}</h3>,
  ul: ({ children }) => <ul className="my-1.5 list-disc space-y-0.5 pl-5">{children}</ul>,
  ol: ({ children }) => <ol className="my-1.5 list-decimal space-y-0.5 pl-5">{children}</ol>,
  li: ({ children }) => <li className="leading-relaxed">{children}</li>,
  a: ({ children, href }) => (
    <a href={href} target="_blank" rel="noreferrer" className="text-indigo-400 underline underline-offset-2 hover:text-indigo-300">
      {children}
    </a>
  ),
  strong: ({ children }) => <strong className="font-semibold text-neutral-100">{children}</strong>,
  em: ({ children }) => <em className="italic">{children}</em>,
  blockquote: ({ children }) => (
    <blockquote className="my-1.5 border-l-2 border-[var(--color-border)] pl-3 text-neutral-400">{children}</blockquote>
  ),
  hr: () => <hr className="my-2 border-[var(--color-border)]" />,
  pre: ({ children }) => (
    <pre className="my-1.5 overflow-auto rounded-md border border-[var(--color-border)] bg-black/40 p-2.5 text-[12px] leading-relaxed text-neutral-200">
      {children}
    </pre>
  ),
  code: ({ className, children, ...props }) => {
    // Fenced blocks carry a `language-*` class; multi-line content is also a
    // block even without a language. Everything else is inline.
    const isBlock = /language-/.test(className ?? "") || String(children).includes("\n");
    if (isBlock) {
      return <code className={className} {...props}>{children}</code>;
    }
    return (
      <code className="rounded bg-black/30 px-1 py-0.5 text-[0.85em] text-amber-200/90" {...props}>
        {children}
      </code>
    );
  },
  table: ({ children }) => (
    <div className="my-1.5 overflow-auto">
      <table className="w-full border-collapse text-xs">{children}</table>
    </div>
  ),
  th: ({ children }) => <th className="border border-[var(--color-border)] px-2 py-1 text-left font-semibold">{children}</th>,
  td: ({ children }) => <td className="border border-[var(--color-border)] px-2 py-1 align-top">{children}</td>,
};

/** Render agent-produced markdown with the app's dark theme. */
export function Markdown({ children }: { children: string }) {
  return (
    <div className="text-sm text-neutral-200">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {children}
      </ReactMarkdown>
    </div>
  );
}
