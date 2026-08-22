/**
 * LSP SymbolKind → a short tag. The names are the LSP ones, abbreviated to
 * stay in one narrow column: the editor's outline lists them in a gutter,
 * and the switcher's `#` mode puts them beside each hit.
 */
import { t } from "../i18n";

const SYMBOL_KINDS: Record<number, string> = {
  1: "symbol.file",
  2: "symbol.module",
  3: "symbol.namespace",
  4: "symbol.package",
  5: "symbol.class",
  6: "symbol.method",
  7: "symbol.property",
  8: "symbol.field",
  9: "symbol.constructor",
  10: "symbol.enum",
  11: "symbol.interface",
  12: "symbol.function",
  13: "symbol.variable",
  14: "symbol.constant",
  15: "symbol.string",
  16: "symbol.number",
  17: "symbol.boolean",
  18: "symbol.array",
  19: "symbol.object",
  20: "symbol.key",
  21: "symbol.null",
  22: "symbol.member",
  23: "symbol.struct",
  24: "symbol.event",
  25: "symbol.operator",
  26: "symbol.typeParameter",
};

/** The tag for an LSP SymbolKind; "?" for values outside the spec. */
export function symbolKindTag(symKind: number): string {
  const key = SYMBOL_KINDS[symKind];
  return key ? t(key) : "?";
}
