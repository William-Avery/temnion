// SPDX-License-Identifier: AGPL-3.0-only

import { useEffect, useRef } from "react";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { sql } from "@codemirror/lang-sql";
import {
  HighlightStyle,
  StreamLanguage,
  bracketMatching,
  syntaxHighlighting,
} from "@codemirror/language";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";

export type QueryLanguage = "temql" | "compact" | "sql";

interface QueryEditorProps {
  ariaLabel: string;
  className?: string;
  language: QueryLanguage;
  onChange: (value: string) => void;
  value: string;
}

const temqlKeywords = /^(?:FROM|ENTITY|TIME|VALID|KNOWN_AS_OF|WHERE|SELECT|LIMIT|EVENT|TRACE|CAUSES|EFFECTS|DEPTH|ORDER|BY|ASC|DESC|AS_OF|PHYSICAL|AND|OR|NOT)\b/i;
const temqlBuiltins = /^(?:temnion|entity|schema|valid_time|known_time|sequence|payload|source|epoch|generation|shard|slot)\b/i;

const temql = StreamLanguage.define({
  token(stream) {
    if (stream.eatSpace()) return null;
    if (stream.match(/^--.*/)) return "comment";
    if (stream.match(/^#(?!\[).*/)) return "comment";
    if (stream.match(/^'(?:[^'\\]|\\.)*(?:'|$)/)) return "string";
    if (stream.match(/^"(?:[^"\\]|\\.)*(?:"|$)/)) return "string";
    if (stream.match(/^\d+(?::\d+){2}\b/)) return "atom";
    if (stream.match(/^(?:0x[\da-f]+|\d+(?:\.\d+)?(?:e[+-]?\d+)?)\b/i)) return "number";
    if (stream.match(/^(?:true|false|null)\b/i)) return "bool";
    if (stream.match(temqlKeywords)) return "keyword";
    if (stream.match(temqlBuiltins)) return "typeName";
    if (stream.match(/^(?:==|!=|>=|<=|>|<|=|\.\.)/)) return "operator";
    if (stream.match(/^[a-z_][\w-]*/i)) return "variableName";
    stream.next();
    return null;
  },
});

const compactTem = StreamLanguage.define({
  token(stream) {
    if (stream.eatSpace()) return null;
    if (stream.sol() && stream.match(/^tn:/i)) return "keyword";
    if (stream.match(/^--.*/)) return "comment";
    if (stream.match(/^'(?:[^'\\]|\\.)*(?:'|$)/)) return "string";
    if (stream.match(/^"(?:[^"\\]|\\.)*(?:"|$)/)) return "string";
    if (stream.match(/^\d+(?::\d+){2}\b/)) return "atom";
    if (stream.match(/^(?:0x[\da-f]+|\d+(?:\.\d+)?)\b/i)) return "number";
    if (stream.match(/^(?:true|false|null)\b/i)) return "bool";
    if (stream.match(/^[><!@?~|&=:+*-]+/)) return "operator";
    if (stream.match(/^[a-z_][\w-]*/i)) return "variableName";
    stream.next();
    return null;
  },
});

const githubDarkHighlight = HighlightStyle.define([
  { tag: tags.comment, color: "#8b949e", fontStyle: "italic" },
  { tag: tags.keyword, color: "#ff7b72" },
  { tag: tags.operator, color: "#ff7b72" },
  { tag: [tags.bool, tags.null, tags.atom], color: "#79c0ff" },
  { tag: tags.number, color: "#79c0ff" },
  { tag: tags.string, color: "#a5d6ff" },
  { tag: tags.typeName, color: "#7ee787" },
  { tag: tags.variableName, color: "#ffa657" },
  { tag: tags.punctuation, color: "#c9d1d9" },
]);

const githubDarkEditor = EditorView.theme(
  {
    "&": {
      backgroundColor: "#0d1117",
      color: "#e6edf3",
      fontFamily: "var(--font-mono)",
      fontSize: "13px",
    },
    ".cm-content": {
      caretColor: "#f0f6fc",
      padding: "12px 0",
    },
    ".cm-line": {
      padding: "0 14px",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "#f0f6fc",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: "#264f78",
    },
    ".cm-activeLine": {
      backgroundColor: "#161b2280",
    },
    ".cm-gutters": {
      backgroundColor: "#0d1117",
      borderRight: "1px solid #21262d",
      color: "#6e7681",
      minWidth: "44px",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      padding: "0 12px 0 8px",
    },
    ".cm-activeLineGutter": {
      backgroundColor: "#161b22",
      color: "#c9d1d9",
    },
    ".cm-scroller": {
      fontFamily: "var(--font-mono)",
      lineHeight: "1.65",
    },
  },
  { dark: true },
);

function languageExtension(language: QueryLanguage): Extension {
  if (language === "sql") return sql();
  if (language === "compact") return compactTem;
  return temql;
}

export function QueryEditor({
  ariaLabel,
  className = "",
  language,
  onChange,
  value,
}: QueryEditorProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const changeHandlerRef = useRef(onChange);
  const languageCompartmentRef = useRef(new Compartment());

  useEffect(() => {
    changeHandlerRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    if (!hostRef.current) return;

    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        history(),
        drawSelection(),
        dropCursor(),
        EditorState.allowMultipleSelections.of(true),
        bracketMatching(),
        highlightActiveLine(),
        keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
        githubDarkEditor,
        syntaxHighlighting(githubDarkHighlight),
        languageCompartmentRef.current.of(languageExtension(language)),
        EditorView.lineWrapping,
        EditorView.contentAttributes.of({
          "aria-label": ariaLabel,
          "aria-multiline": "true",
          autocapitalize: "off",
          autocomplete: "off",
          spellcheck: "false",
        }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            changeHandlerRef.current(update.state.doc.toString());
          }
        }),
      ],
    });

    const view = new EditorView({ state, parent: hostRef.current });
    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const currentValue = view.state.doc.toString();
    if (currentValue !== value) {
      view.dispatch({
        changes: { from: 0, to: currentValue.length, insert: value },
      });
    }
  }, [value]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: languageCompartmentRef.current.reconfigure(languageExtension(language)),
    });
  }, [language]);

  return (
    <div
      className={`query-code-editor ${className}`.trim()}
      data-language={language}
      ref={hostRef}
    />
  );
}
