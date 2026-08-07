/**
 * A3 (Amino Acid Annotation) format — browser entry point.
 *
 * Identical to the main entry point except that file I/O (`readJSON`,
 * `writeJSON`) is excluded, as those depend on `node:fs/promises`.
 * Safe to use in browsers, Deno, and edge runtimes.
 *
 * @module
 */

export { A3, A3ParseError, A3ValidationError } from "./a3";
export type { A3Issue, A3IssueCode, A3Stage } from "./issues";
export {
  A3_ISSUE_CODES,
  A3_ISSUE_STAGES,
  escapeToken,
  pointer,
  resolvePointer,
  stageOf,
} from "./issues";
export type { ParsedIndex, ParseResult } from "./parse";
export {
  formatPositions,
  formatRanges,
  parseIndex,
  parsePositions,
  parseRanges,
} from "./parse";
export type {
  A3Data,
  A3FlexData,
  A3PositionData,
  A3RangeData,
  MetadataData,
  VariantData,
} from "./schemas";
export { A3_SCHEMA_URI, A3_VERSION } from "./schemas";
export type { ValidationResult } from "./validate";
export { validate } from "./validate";
