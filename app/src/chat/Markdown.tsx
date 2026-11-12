import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "../ipc";

// Links never navigate the panel: https ones open in the default browser, others do nothing.
const components: Components = {
  a: ({ href, children }) => (
    <a
      href={href}
      title={href}
      onClick={(e) => {
        e.preventDefault();
        if (href?.startsWith("https://")) void api.openUrl(href);
      }}
    >
      {children}
    </a>
  ),
  img: ({ alt }) => <span>{alt}</span>,
};

/** bleep's reply text. Raw HTML in it stays text (react-markdown's default). */
export function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>{text}</ReactMarkdown>
    </div>
  );
}
