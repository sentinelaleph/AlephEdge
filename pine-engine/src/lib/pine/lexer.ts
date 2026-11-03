/**
 * Pine-lite lexer: source text -> tokens.
 *
 * Indentation (Pine rule): a line indented by a multiple of 4 columns (a tab
 * counts as 4) deeper than the current block opens a block (INDENT); a line
 * indented deeper by any other amount continues the previous line. Inside
 * ( ) and [ ] newlines and indentation are ignored, so wrapped call
 * arguments work in v5 and v6 alike. As a lenient superset, a deeper line
 * that follows a binary operator, comma, `?`, `:` or `=` also continues the
 * previous line even when its indent is a multiple of 4.
 */
import { fail } from "./errors";

export type TokType = "num" | "str" | "color" | "ident" | "op" | "nl" | "indent" | "dedent" | "eof";

export interface Token {
  t: TokType;
  v: string;
  line: number;
  col: number;
  /** Numeric value for "num". */
  n?: number;
  /** "num" written without a decimal point or exponent. */
  int?: boolean;
}

export interface LexResult {
  tokens: Token[];
  version: number | null;
  versionPos: { line: number; col: number } | null;
}

const OPS2 = new Set([":=", "+=", "-=", "*=", "/=", "%=", "==", "!=", "<=", ">=", "=>"]);
const OPS1 = new Set(["+", "-", "*", "/", "%", "<", ">", "=", "?", ":", "(", ")", "[", "]", ",", "."]);
/** A line ending in one of these cannot end a statement. */
const CONTINUES = new Set(["+", "-", "*", "/", "%", "<", ">", "<=", ">=", "==", "!=", "=", ":=", "?", ":", ",", "and", "or", "not", "+=", "-=", "*=", "/=", "%="]);

const isDigit = (c: string) => c >= "0" && c <= "9";
const isIdStart = (c: string) => (c >= "a" && c <= "z") || (c >= "A" && c <= "Z") || c === "_";
const isIdChar = (c: string) => isIdStart(c) || isDigit(c);
const isHex = (c: string) => isDigit(c) || (c >= "a" && c <= "f") || (c >= "A" && c <= "F");

export function lex(src: string): LexResult {
  const tokens: Token[] = [];
  const stack = [0];
  let version: number | null = null;
  let versionPos: { line: number; col: number } | null = null;
  let i = 0;
  let line = 1;
  let lineStart = 0;
  let depth = 0;
  let atLineStart = true;
  const n = src.length;
  const push = (t: TokType, v: string, col: number, extra?: Partial<Token>) => tokens.push({ t, v, line, col, ...extra });

  while (i < n) {
    if (atLineStart) {
      atLineStart = false;
      // Measure the indent of this physical line.
      let width = 0;
      let j = i;
      while (j < n && (src[j] === " " || src[j] === "\t")) {
        width += src[j] === "\t" ? 4 : 1;
        j++;
      }
      const rest = src.slice(j, j + 2);
      const blank = j >= n || src[j] === "\n" || src[j] === "\r" || rest === "//";
      i = j;
      if (!blank && depth === 0 && tokens.length > 0) layout(width, j - lineStart + 1);
      continue;
    }
    const c = src[i];
    const col = i - lineStart + 1;
    if (c === "\n") {
      i++;
      line++;
      lineStart = i;
      atLineStart = true;
      continue;
    }
    if (c === " " || c === "\t" || c === "\r") {
      i++;
      continue;
    }
    if (c === "/" && src[i + 1] === "/") {
      let j = i;
      while (j < n && src[j] !== "\n") j++;
      const text = src.slice(i, j);
      const m = /^\/\/@version\s*=\s*(\d+)/.exec(text);
      if (m && version === null) {
        version = Number(m[1]);
        versionPos = { line, col };
      }
      i = j;
      continue;
    }
    if (isDigit(c) || (c === "." && isDigit(src[i + 1] ?? ""))) {
      let j = i;
      let int = true;
      while (j < n && isDigit(src[j])) j++;
      if (src[j] === "." && !isIdStart(src[j + 1] ?? "")) {
        int = false;
        j++;
        while (j < n && isDigit(src[j])) j++;
      }
      if ((src[j] === "e" || src[j] === "E") && (isDigit(src[j + 1] ?? "") || ((src[j + 1] === "+" || src[j + 1] === "-") && isDigit(src[j + 2] ?? "")))) {
        int = false;
        j += 2;
        while (j < n && isDigit(src[j])) j++;
      }
      const text = src.slice(i, j);
      push("num", text, col, { n: Number(text), int });
      i = j;
      continue;
    }
    if (isIdStart(c)) {
      let j = i;
      while (j < n && isIdChar(src[j])) j++;
      push("ident", src.slice(i, j), col);
      i = j;
      continue;
    }
    if (c === '"' || c === "'") {
      i = lexString(src, i, c, line, col, (v) => push("str", v, col));
      continue;
    }
    if (c === "#") {
      let j = i + 1;
      while (j < n && isHex(src[j])) j++;
      const len = j - i - 1;
      if (len !== 6 && len !== 8) fail("syntax", "Colour literals must be #RRGGBB or #RRGGBBAA", { line, col });
      push("color", src.slice(i, j).toUpperCase(), col);
      i = j;
      continue;
    }
    const two = src.slice(i, i + 2);
    const op = OPS2.has(two) ? two : OPS1.has(c) ? c : null;
    if (op === null) fail("syntax", `Unexpected character '${c}'`, { line, col });
    if (op === "(" || op === "[") depth++;
    if ((op === ")" || op === "]") && depth > 0) depth--;
    push("op", op, col);
    i += op.length;
  }
  push("nl", "", i - lineStart + 1);
  while (stack.length > 1) {
    stack.pop();
    push("dedent", "", 1);
  }
  push("eof", "", i - lineStart + 1);
  return { tokens, version, versionPos };

  function layout(width: number, col: number) {
    const cur = stack[stack.length - 1];
    const last = tokens[tokens.length - 1];
    if (width > cur) {
      if (width % 4 !== 0 || ((last.t === "op" || last.t === "ident") && CONTINUES.has(last.v))) return; // continuation
      push("nl", "", col);
      stack.push(width);
      push("indent", "", col);
      return;
    }
    push("nl", "", col);
    while (width < stack[stack.length - 1]) {
      stack.pop();
      push("dedent", "", col);
    }
    if (width !== stack[stack.length - 1]) fail("syntax", "Indentation does not match any enclosing block", { line, col });
  }
}

function lexString(src: string, start: number, quote: string, line: number, col: number, emit: (v: string) => void): number {
  let j = start + 1;
  let out = "";
  while (j < src.length && src[j] !== quote) {
    const ch = src[j];
    if (ch === "\n") break;
    if (ch === "\\") {
      const e = src[j + 1];
      out += e === "n" ? "\n" : e === "t" ? "\t" : e === "r" ? "\r" : e ?? "";
      j += 2;
      continue;
    }
    out += ch;
    j++;
  }
  if (src[j] !== quote) fail("syntax", "Unterminated string literal", { line, col });
  emit(out);
  return j + 1;
}

