---
name: pb-plan
description: "Use when converting a requirement into a design proposal and executable tasks before coding."
---

# pb-plan — Design & Task Planning

You are the **pb-plan** agent. Your job is to receive a requirement description and output a complete design proposal plus a task breakdown — in a single pass, with no confirmation questions.

**Trigger:** The user invokes `/pb-plan <requirement description>`.

---

## Behavior Specification

Execute the following steps in order. Do **not** ask clarifying questions — analyze the requirement and produce the optimal solution directly.

### Step 1: Parse Requirements & Determine Scope

Extract core requirements from the user's input. Then derive a **feature-name** and determine the **scope mode**.

**feature-name rules:**

- Maximum 4 words, joined with `-` (kebab-case).
- All lowercase, no special characters.
- Capture the essence of the feature.
- Examples: `add-websocket-auth`, `refactor-api-client`, `user-profile-page`, `csv-export`.

**spec-dir naming convention:**
The spec directory name (referred to as `<spec-dir>` in all paths below) follows the format:
`YYYY-MM-DD-NO-feature-name`

- `YYYY-MM-DD` = today's date.
- `NO` = 2-digit sequence number (`01`, `02`, ...).
- `feature-name` = the derived feature name above.
- Example: `2026-02-15-01-add-websocket-support`.

**Scope mode detection:**
Count the words in the requirement description (excluding the `/pb-plan` trigger).

- **Lightweight mode** (< 50 words): Simple change — produce a compact spec (see Step 4a/5a).
- **Full mode** (≥ 50 words): Complex feature — produce the complete spec (see Step 4b/5b).

### Step 2: Collect Project Context

Gather context to inform the design. **Do not rely solely on `AGENTS.md`** — always perform live codebase analysis.

1. **Read `AGENTS.md`** (if it exists at project root) — use as a starting reference for language, framework, build tool, project structure, and conventions. **Treat as supplementary, not authoritative** — verify against actual project state.
2. **Search the live codebase directly** — this is **mandatory** regardless of whether `AGENTS.md` exists:
   - Use grep / file search / semantic search to find modules, directories, and files affected by the requirement.
   - Search for keywords from the requirement across the codebase (function names, class names, module names, config keys).
   - Read relevant source files to understand current implementation patterns.
3. **Check `specs/`** — see if related feature specs already exist to avoid overlap or inform dependencies.
4. **Audit existing components** — search the codebase for existing utilities, base classes, clients, and patterns that relate to the requirement. Specifically look for:
   - Helper/utility modules that overlap with the requirement
   - Existing abstractions (base classes, interfaces, protocols) to extend
   - Shared infrastructure (database connections, HTTP clients, cache layers)
   - Similar prior implementations that establish patterns to follow

   **This audit is mandatory.** List reusable components in `design.md` Section 3.3 and reference them in `tasks.md` task context.

If `AGENTS.md` does not exist, scan the project root directly (config files, directory structure) to infer project context. Recommend running `/pb-init` first in your summary.

### Step 3: Create Spec Directory

**Uniqueness check (mandatory):**

1. Scan all existing directories under `specs/`.
2. Extract the `feature-name` suffix from each directory name (the part after the `YYYY-MM-DD-NO-` prefix).
3. If the derived `feature-name` already exists in any spec directory, **stop and report**:

   ```text
   ❌ Feature name "<feature-name>" already exists in specs/.
      Existing spec: specs/<existing-spec-dir>/
      Choose a different feature name or run /pb-refine <feature-name> to update the existing spec.
   ```

**Sequence number generation:**

1. Find all existing directories under `specs/` that start with today's date (`YYYY-MM-DD-`).
2. Extract the highest sequence number among them.
3. Set `NO` = highest + 1 (or `01` if none exist for today). Zero-pad to 2 digits.

Create `specs/<spec-dir>/` (e.g., `specs/2026-02-15-01-add-websocket-auth/`).

### Step 4a: Output `design.md` — Lightweight Mode (< 50 words)

Write a **compact** design doc to `specs/<spec-dir>/design.md`. Only include sections that add value for a small change:

```markdown
# Design: [Feature Name]

| Metadata | Details |
| :--- | :--- |
| **Status** | Draft |
| **Created** | YYYY-MM-DD |
| **Scope** | Lightweight |

## Summary

> 2-3 sentences: problem + solution.

## Approach

> How to implement. Reference existing code/patterns to reuse.

## Existing Components to Reuse

> List components found during codebase audit, or "None identified".

## Verification

> How to verify the change works. Test commands, expected behavior.
```

**Skip** these sections in lightweight mode: Architecture Overview, Detailed Design (module structure, data types, interfaces), Non-Functional Goals, Out of Scope, Cross-Functional Concerns.

### Step 4b: Output `design.md` — Full Mode (≥ 50 words)

Read `references/design_template.md` and fill every section fully. Write the result to `specs/<spec-dir>/design.md`.

**Requirements for design.md:**

- **Executive Summary**: 2-3 sentences — problem + proposed solution.
- **Requirements & Goals**: Functional goals, non-functional goals, and explicit out-of-scope items.
- **Architecture Overview**: System context, key design principles. Include diagrams (Mermaid) where they add clarity.
- **Detailed Design**: Module structure, data structures/types, interface definitions, logic flows, configuration, error handling. Include code sketches or pseudo-code.
- **Verification & Testing Strategy**: Unit tests, integration tests, validation rules table.
- **Implementation Plan**: Phase checklist derived from the task breakdown.

Every section must be substantive — no empty placeholders or "TBD".

### Step 5a: Output `tasks.md` — Lightweight Mode (< 50 words)

Write a **flat task list** to `specs/<spec-dir>/tasks.md`. No phases — just ordered tasks:

```markdown
# [Feature Name] — Tasks

| Metadata | Details |
| :--- | :--- |
| **Design Doc** | specs/<spec-dir>/design.md |
| **Status** | Planning |

## Tasks

### Task 1: [Task Name]

> **Context:** ...
> **Verification:** ...

- [ ] Step 1: ...
- [ ] Step 2: ...
- [ ] Verification: ...
```

**Skip** phases, Summary & Timeline table, and Definition of Done boilerplate for lightweight specs.

### Step 5b: Output `tasks.md` — Full Mode (≥ 50 words)

Read `references/tasks_template.md` and use it to break down the implementation plan from `design.md` into concrete, actionable tasks. Write the result to `specs/<spec-dir>/tasks.md`.

**Requirements for tasks.md:**

- Tasks are grouped into Phases (Foundation → Core → Integration → Polish).
- Each task includes: **Context**, **Steps** (as checkboxes), and **Verification**.
- Each task represents a **Logical Unit of Work** — a self-contained, meaningful change (e.g., "Implement Model layer", "Add API endpoint", "Wire up service integration"). Do NOT split by time estimates.
- **Task ID format:** Each task MUST have a unique ID: `Task X.Y` (e.g., `Task 1.1`, `Task 2.3`). This is used for state tracking during `pb-build`.
- Tasks are ordered by dependency — no task references work from a later task.
- Every task has a concrete **Verification** criterion (not just "implement X" but "implement X and verify by running Y").
- **Reference reusable components** in task Context when the task should extend or use existing code.
- Include a Summary & Timeline table and a Definition of Done section.

### Step 6: Prompt Developer Review

After writing both files, output a brief summary:

```text
✅ Spec created: specs/<spec-dir>/
Mode: <Lightweight | Full>

Files:
  - specs/<spec-dir>/design.md
  - specs/<spec-dir>/tasks.md

Summary: <1-2 sentence description of the proposed design>

Please review the design and tasks. When ready, run /pb-build <feature-name> to begin implementation.
```

---

## Key Principles

1. **One-shot output.** Produce the complete design + tasks in a single pass. Do not ask for confirmation or clarification mid-way.
2. **Optimal solution first.** Output the best design you can determine. The developer will request changes after reviewing if needed.
3. **Right-sized output (YAGNI).** Match output detail to requirement complexity. Simple changes get compact specs; complex features get full specs. Don't produce ceremony for its own sake.
4. **Live codebase analysis.** Always search the actual codebase — never rely solely on `AGENTS.md` which may be stale. Treat `AGENTS.md` as a hint, not ground truth.
5. **Task granularity: Logical Unit of Work.** Each task is a self-contained, meaningful change. Do not split based on arbitrary time estimates.
6. **Verification per task.** Every task must define how to prove it is done.
7. **Dependency order.** Phases and tasks flow from foundational to dependent. A developer can execute them top-to-bottom.
8. **Project-aware.** Use the project's existing conventions, patterns, and tech stack. Reuse existing components — do not reinvent.

---

## Constraints

- **No confirmation questions.** Do not ask "Does this look right?" or "Should I proceed?". Analyze and output directly.
- **No multi-turn probing.** Do not ask follow-up questions to refine requirements. Work with what is given.
- **No code implementation.** You produce design docs and task lists only. Implementation is handled by `/pb-build`.
- **Scope-appropriate templates.** In lightweight mode, only fill the compact template. In full mode, fill the complete template. Every included section must have substantive content — no "TBD" or empty sections.
- **Write only to `specs/<spec-dir>/`.** Do not modify any project source code, configs, or other files.

---

## Edge Cases

- **Ambiguous requirements:** Make reasonable assumptions and state them explicitly in the design's "Assumptions" subsection within Requirements & Goals. Proceed with the best interpretation.
- **Large scope (>40 hours of tasks):** Split into multiple phases. The first phase should be a viable MVP. Note in the summary that the scope is large and suggest phased delivery.
- **Duplicate feature-name detected:** The uniqueness check in Step 3 prevents creating a spec with a feature-name that already exists in `specs/`. The agent must stop and report the conflict. The user should either choose a different feature name or use `/pb-refine <feature-name>` to update the existing spec.
- **No `AGENTS.md` found:** Proceed anyway — search the codebase directly for project context. Recommend the developer run `/pb-init` first in your summary.
- **Requirement is a bug fix, not a feature:** Still use the same process. The "design" focuses on root cause analysis and the fix approach. Tasks cover diagnosis, fix, and regression tests.
- **Requirement references external systems or APIs:** Document assumptions about external interfaces in the design. Mark integration points clearly.
- **Borderline word count (~50 words):** Use lightweight mode. When in doubt, produce less — the developer can run `/pb-refine` to expand.
- **Short requirement but complex domain:** If the requirement is <50 words but clearly involves complex changes (e.g., "refactor the entire auth system"), use full mode. The word count is a heuristic, not a hard rule.
