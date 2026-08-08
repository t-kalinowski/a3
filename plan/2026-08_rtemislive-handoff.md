# Handoff: updating rtemislive for `@rtemis/a3` 0.3.0

**Status:** ready to apply, not applied — `live/` is a separate repository.
**Target:** `live/src/components/a3doc/issueUtils.ts` and its callers.

## What changed upstream

`@rtemis/a3` 0.3.0 replaced Zod with a hand-written staged validator so that all
five A3 implementations produce byte-identical `{code, path}` pairs. Two things
that rtemislive depends on changed:

1. **`A3ValidationIssue` is now `A3Issue`**, and its `path` is a **string** — an
   RFC 6901 JSON Pointer — not a `readonly PropertyKey[]`.
2. **Every issue carries a stable `code`** from a fixed registry
   (`spec/error-codes.md`), so nothing needs to parse messages any more.

`A3InputSchema` is gone. If any form uses `zodResolver(A3InputSchema)`, replace
it with a resolver built from `validate()` — see the last section.

## Why this is a strict improvement

The current `issueUtils.ts` compares path *segments* against literals. That
works, but it means every call site spells out a path as an array
(`["annotations", "site", name, "index"]`), and the segment types have to be
reconciled between `string`, `number`, and `symbol`. With pointers, a path is
one string, comparison is `===`, and prefix matching is `startsWith`.

More importantly, the pointers are contractual. A path that appears in a
rtemislive test is the same path Rust, Python, Julia, and R produce for the same
document, so the form's field mapping is pinned by the same corpus that pins the
libraries.

## Replacement `issueUtils.ts`

```ts
import { type A3Issue, escapeToken, pointer } from "@rtemis/a3";

/**
 * Build an RFC 6901 JSON Pointer from path segments.
 *
 * Prefer this over string concatenation: annotation names are user-supplied and
 * may contain `/` or `~`, which must be escaped as `~1` and `~0`.
 */
export function path(...segments: (string | number)[]): string {
  return segments.reduce<string>((acc, seg) => pointer(acc, seg), "");
}

/** The first issue reported at exactly `target`. */
export function issueAtPath(
  issues: readonly A3Issue[],
  target: string,
): A3Issue | undefined {
  return issues.find((i) => i.path === target);
}

/**
 * Every issue at or below `prefix`.
 *
 * The trailing-separator check keeps `/annotations/site/ab` from matching a
 * prefix of `/annotations/site/a`.
 */
export function issuesUnder(
  issues: readonly A3Issue[],
  prefix: string,
): A3Issue[] {
  return issues.filter(
    (i) => i.path === prefix || i.path.startsWith(`${prefix}/`),
  );
}

/** Every issue carrying `code`, regardless of where it was reported. */
export function issuesWithCode(
  issues: readonly A3Issue[],
  code: A3Issue["code"],
): A3Issue[] {
  return issues.filter((i) => i.code === code);
}
```

`escapeToken` is re-exported for the rare case where a caller builds a pointer
fragment by hand; `path()` covers everything else.

## Call-site migration

| Before | After |
|---|---|
| `issueAtPath(issues, ["sequence"])` | `issueAtPath(issues, "/sequence")` |
| `issuesUnder(issues, ["annotations", "site", name])` | `issuesUnder(issues, path("annotations", "site", name))` |
| `issueAtPath(issues, ["annotations", "variant", i, "position"])` | `issueAtPath(issues, path("annotations", "variant", i, "position"))` |
| `issue.message.includes("out of bounds")` | `issue.code === "A3E_POS_OUT_OF_BOUNDS"` |

The last row is the one that matters. Message matching breaks whenever a message
is reworded, and messages are explicitly **not** part of the A3 contract —
they are expected to change.

## Getting the issues

`new A3(input)` still throws. For a live-validation panel, use the non-throwing
entry point instead:

```ts
import { validate } from "@rtemis/a3";

const { data, issues } = validate(parsed);
// data is null when issues is non-empty
```

Two behaviors worth designing around:

- **Issues come from one stage at a time.** Validation short-circuits at a stage
  boundary, so a document with a malformed `sequence` reports *only* that, not
  the hundreds of out-of-bounds positions that follow from it. After the user
  fixes the sequence, the next pass may surface a fresh batch. A panel that says
  "3 problems" should say "3 problems at this stage" or show `issues[0].stage`.
- **`annotations` and `metadata` are optional.** A document of just `$schema`,
  `a3_version`, and `sequence` is valid. Any form logic that assumed those keys
  exist should default them rather than flag them.

## Replacing `zodResolver(A3InputSchema)`

react-hook-form resolvers are plain functions, so this is ~20 lines and produces
better errors than the Zod version did, because it keys on contractual paths:

```ts
import type { Resolver } from "react-hook-form";
import { validate } from "@rtemis/a3";

export const a3Resolver: Resolver<A3FormValues> = (values) => {
  const { data, issues } = validate(values);
  if (data !== null) return { values: data, errors: {} };

  const errors: Record<string, { type: string; message: string }> = {};
  for (const issue of issues) {
    // "/annotations/site/Active site/index/2" -> "annotations.site.Active site.index.2"
    // Adjust to whatever field-name scheme the form registers.
    const field = issue.path.slice(1).split("/").join(".");
    errors[field] ??= { type: issue.code, message: issue.message };
  }
  return { values: {}, errors };
};
```

Note `??=`: the first issue at a field wins, matching react-hook-form's
one-error-per-field model. If a field can carry several issues, collect them in
`message` instead.

## Checklist

- [ ] Bump `@rtemis/a3` to `^0.3.0` (npm) — `zod` may now be removable from
      rtemislive too, if nothing else uses it
- [ ] Replace `issueUtils.ts` with the version above
- [ ] Update every call site per the table
- [ ] Replace any `A3InputSchema` / `zodResolver` usage
- [ ] Grep for `.message.includes(` and `.message.match(` on issues — every hit
      is a latent break
- [ ] Grep for `A3ValidationIssue` — the type is now `A3Issue`
- [ ] Check that form defaults tolerate a document with no `annotations` or
      `metadata`
