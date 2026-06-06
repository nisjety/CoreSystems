import { Fragment, type ReactNode } from "react";
import { cn } from "@/lib/utils";

type ChatMarkdownProps = {
  content: string;
  className?: string;
};

/**
 * Zero-dependency markdown renderer for assistant messages. Supports the
 * subset the chat needs — headings, bold/italic, inline + fenced code, links,
 * nested ordered/unordered lists, blockquotes, and horizontal rules — with a
 * clean reading-optimised prose style. Streaming-safe: partial/incomplete
 * markdown degrades gracefully to text.
 *
 * Intentionally avoids react-markdown/remark so the chat never depends on an
 * external package being resolvable in every build environment.
 */
export function ChatMarkdown({ content, className }: ChatMarkdownProps) {
  return (
    <div
      className={cn(
        "velion-chat-prose text-[15px] leading-[1.68] text-[#24262D] dark:text-[#F4F6F8]",
        className,
      )}
    >
      {renderBlocks(content)}
    </div>
  );
}

const INLINE_CODE_CLASS =
  "rounded-[6px] border border-[#E9E9EC] bg-[#F5F5F4] px-1.5 py-0.5 font-mono text-[13px] text-[#3A3D45] dark:border-[#2C2E34] dark:bg-[#1B1C21] dark:text-[#D6DAE2]";

// ---- Block level -----------------------------------------------------------

function renderBlocks(markdown: string): ReactNode[] {
  const lines = markdown.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let index = 0;
  let key = 0;

  while (index < lines.length) {
    const line = lines[index];

    if (line.trim() === "") {
      index += 1;
      continue;
    }

    // Fenced code block
    const fence = /^(\s*)```(.*)$/.exec(line);
    if (fence) {
      const lang = fence[2].trim();
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !/^\s*```/.test(lines[index])) {
        code.push(lines[index]);
        index += 1;
      }
      index += 1; // consume closing fence
      blocks.push(
        <pre
          key={key++}
          className="my-3 overflow-x-auto rounded-[12px] border border-[#ECECEF] bg-[#FAFAF9] p-3.5 dark:border-[#2A2C32] dark:bg-[#16171B]"
        >
          <code className={cn("font-mono text-[13px] leading-relaxed", lang && `language-${lang}`)}>
            {code.join("\n")}
          </code>
        </pre>,
      );
      continue;
    }

    // Horizontal rule
    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      blocks.push(<hr key={key++} className="my-5 border-t border-[#ECECEF] dark:border-[#2A2C32]" />);
      index += 1;
      continue;
    }

    // Heading
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      blocks.push(renderHeading(heading[1].length, heading[2], key++));
      index += 1;
      continue;
    }

    // Blockquote (consecutive `>` lines, may contain nested blocks)
    if (/^\s*>\s?/.test(line)) {
      const quoted: string[] = [];
      while (index < lines.length && /^\s*>\s?/.test(lines[index])) {
        quoted.push(lines[index].replace(/^\s*>\s?/, ""));
        index += 1;
      }
      blocks.push(
        <blockquote
          key={key++}
          className="my-3 border-l-2 border-[#E3E2DF] pl-4 text-[#5C606B] dark:border-[#34373E] dark:text-[#A8AEB9]"
        >
          {renderBlocks(quoted.join("\n"))}
        </blockquote>,
      );
      continue;
    }

    // GFM table (header row immediately followed by a delimiter row)
    if (isTableAt(lines, index)) {
      const { node, next } = renderTable(lines, index, key++);
      blocks.push(node);
      index = next;
      continue;
    }

    // List (ordered or unordered, with nesting)
    if (getListItem(line)) {
      const { node, next } = renderList(lines, index, key++);
      blocks.push(node);
      index = next;
      continue;
    }

    // Paragraph (collect consecutive non-block lines)
    const paragraph: string[] = [];
    while (
      index < lines.length &&
      lines[index].trim() !== "" &&
      !isBlockStart(lines[index]) &&
      !isTableAt(lines, index)
    ) {
      paragraph.push(lines[index]);
      index += 1;
    }
    blocks.push(
      <p key={key++} className="mb-3 last:mb-0">
        {parseInline(paragraph.join("\n").trim())}
      </p>,
    );
  }

  return blocks;
}

function isBlockStart(line: string): boolean {
  return (
    /^\s*```/.test(line) ||
    /^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line) ||
    /^#{1,6}\s+/.test(line) ||
    /^\s*>\s?/.test(line) ||
    getListItem(line) !== null
  );
}

function renderHeading(level: number, text: string, key: number): ReactNode {
  const children = parseInline(text);
  if (level <= 1) {
    return (
      <h1 key={key} className="mb-3 mt-5 text-[19px] font-semibold leading-snug text-[#191A1F] first:mt-0 dark:text-white">
        {children}
      </h1>
    );
  }
  if (level === 2) {
    return (
      <h2 key={key} className="mb-2.5 mt-5 text-[17px] font-semibold leading-snug text-[#191A1F] first:mt-0 dark:text-white">
        {children}
      </h2>
    );
  }
  return (
    <h3 key={key} className="mb-2 mt-4 text-[15px] font-semibold leading-snug text-[#191A1F] first:mt-0 dark:text-white">
      {children}
    </h3>
  );
}

// ---- Tables (GitHub-flavoured) ---------------------------------------------

type ColumnAlign = "left" | "center" | "right" | "default";

function splitTableRow(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split(/(?<!\\)\|/)
    .map((cell) => cell.trim().replace(/\\\|/g, "|"));
}

function isDelimiterRow(line: string): boolean {
  if (!line.includes("|") && !/^\s*:?-{2,}:?\s*$/.test(line)) {
    return false;
  }
  const cells = splitTableRow(line);
  return cells.length > 0 && cells.every((cell) => /^:?-{1,}:?$/.test(cell));
}

function isTableAt(lines: string[], index: number): boolean {
  const header = lines[index];
  const delimiter = lines[index + 1];
  if (header === undefined || delimiter === undefined) {
    return false;
  }
  return header.includes("|") && isDelimiterRow(delimiter);
}

function columnAlign(cell: string): ColumnAlign {
  const left = cell.startsWith(":");
  const right = cell.endsWith(":");
  if (left && right) return "center";
  if (right) return "right";
  if (left) return "left";
  return "default";
}

function alignClass(align: ColumnAlign): string {
  if (align === "center") return "text-center";
  if (align === "right") return "text-right";
  return "text-left";
}

function renderTable(
  lines: string[],
  start: number,
  key: number,
): { node: ReactNode; next: number } {
  const header = splitTableRow(lines[start]);
  const aligns = splitTableRow(lines[start + 1]).map(columnAlign);
  const columnCount = header.length;

  let index = start + 2;
  const rows: string[][] = [];
  while (
    index < lines.length &&
    lines[index].trim() !== "" &&
    lines[index].includes("|") &&
    !isDelimiterRow(lines[index])
  ) {
    rows.push(splitTableRow(lines[index]));
    index += 1;
  }

  const cellAlign = (column: number): ColumnAlign => aligns[column] ?? "default";

  const node = (
    <div key={key} className="my-3 overflow-x-auto">
      <table className="w-full border-collapse text-[14px]">
        <thead className="border-b border-[#E5E5E8] text-[#5C606B] dark:border-[#2C2E34] dark:text-[#A8AEB9]">
          <tr>
            {header.map((cell, column) => (
              <th
                key={column}
                className={cn("px-3 py-2 font-semibold", alignClass(cellAlign(column)))}
              >
                {parseInline(cell)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, rowIndex) => (
            <tr key={rowIndex}>
              {Array.from({ length: columnCount }).map((_, column) => (
                <td
                  key={column}
                  className={cn(
                    "border-b border-[#F0F0F2] px-3 py-2 dark:border-[#23252B]",
                    alignClass(cellAlign(column)),
                  )}
                >
                  {parseInline(row[column] ?? "")}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );

  return { node, next: index };
}

// ---- Lists -----------------------------------------------------------------

type ListItemMatch = { indent: number; ordered: boolean; text: string };

function getListItem(line: string): ListItemMatch | null {
  const match = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/.exec(line);
  if (!match) {
    return null;
  }
  return {
    indent: match[1].replace(/\t/g, "  ").length,
    ordered: /\d/.test(match[2]),
    text: match[3],
  };
}

type ListItemNode = { text: string; children: ReactNode[] };

function renderList(
  lines: string[],
  start: number,
  key: number,
): { node: ReactNode; next: number } {
  const first = getListItem(lines[start])!;
  const baseIndent = first.indent;
  const ordered = first.ordered;
  const items: ListItemNode[] = [];
  let index = start;

  while (index < lines.length) {
    const item = getListItem(lines[index]);
    if (!item || item.indent < baseIndent) {
      break;
    }

    if (item.indent > baseIndent) {
      // Deeper indent → a nested list attached to the previous item.
      const { node, next } = renderList(lines, index, 0);
      if (items.length > 0) {
        items[items.length - 1].children.push(node);
      }
      index = next;
      continue;
    }

    if (item.ordered !== ordered) {
      break;
    }

    items.push({ text: item.text, children: [] });
    index += 1;
  }

  const renderedItems = items.map((item, itemIndex) => (
    <li key={itemIndex} className="pl-1">
      {parseInline(item.text)}
      {item.children.map((child, childIndex) => (
        <Fragment key={childIndex}>{child}</Fragment>
      ))}
    </li>
  ));

  const node = ordered ? (
    <ol
      key={key}
      className="mb-3 list-decimal space-y-1.5 pl-5 marker:font-medium marker:text-[#9AA0A9] last:mb-0 dark:marker:text-[#7A808B]"
    >
      {renderedItems}
    </ol>
  ) : (
    <ul
      key={key}
      className="mb-3 list-disc space-y-1.5 pl-5 marker:text-[#BFC3CB] last:mb-0 dark:marker:text-[#5A616E]"
    >
      {renderedItems}
    </ul>
  );

  return { node, next: index };
}

// ---- Inline ----------------------------------------------------------------

const INLINE_PATTERN =
  /(`+)([\s\S]+?)\1|\*\*([\s\S]+?)\*\*|\*([\s\S]+?)\*|\[([^\]]+)\]\(([^)\s]+)\)/g;

function parseInline(text: string): ReactNode {
  if (!text) {
    return null;
  }

  const nodes: ReactNode[] = [];
  let lastIndex = 0;
  let key = 0;
  let match: RegExpExecArray | null;

  INLINE_PATTERN.lastIndex = 0;
  while ((match = INLINE_PATTERN.exec(text)) !== null) {
    if (match.index > lastIndex) {
      nodes.push(<Fragment key={key++}>{text.slice(lastIndex, match.index)}</Fragment>);
    }

    if (match[2] !== undefined) {
      nodes.push(
        <code key={key++} className={INLINE_CODE_CLASS}>
          {match[2]}
        </code>,
      );
    } else if (match[3] !== undefined) {
      nodes.push(
        <strong key={key++} className="font-semibold text-[#191A1F] dark:text-white">
          {parseInline(match[3])}
        </strong>,
      );
    } else if (match[4] !== undefined) {
      nodes.push(
        <em key={key++} className="italic">
          {parseInline(match[4])}
        </em>,
      );
    } else if (match[5] !== undefined) {
      nodes.push(
        <a
          key={key++}
          href={match[6]}
          target="_blank"
          rel="noopener noreferrer"
          className="font-medium text-[#3578F6] underline-offset-2 hover:underline dark:text-[#7FB0FF]"
        >
          {parseInline(match[5])}
        </a>,
      );
    }

    lastIndex = INLINE_PATTERN.lastIndex;
  }

  if (lastIndex < text.length) {
    nodes.push(<Fragment key={key++}>{text.slice(lastIndex)}</Fragment>);
  }

  return nodes;
}
