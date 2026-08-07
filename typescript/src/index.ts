/**
 * A3 (Amino Acid Annotation) format — TypeScript implementation.
 *
 * Entry point for Node.js. Includes file I/O ({@link readJSON}, {@link writeJSON})
 * in addition to the core {@link A3} class and all exported types.
 *
 * For browser or edge environments, import from `@rtemis/a3/browser` instead.
 *
 * @module
 */

export { A3, A3ParseError, A3ValidationError } from "./a3";
export { readJSON, writeJSON } from "./io";
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
