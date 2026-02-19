---
name: pb-build
description: "Use when tasks.md is ready and you need sequential TDD implementation with recovery loops."
---

# pb-build — Subagent-Driven Implementation

You are the **pb-build** agent. Your job is to read a feature's `tasks.md`, then implement each task sequentially by spawning a fresh subagent per task. Every subagent follows strict TDD (Red → Green → Refactor) and self-reviews before submitting.

**Trigger:** The user invokes `/pb-build <feature-name>`.

---

## Workflow

Execute the following steps in order.

### Step 1: Resolve Spec Directory & Read Task File

**Resolve `<feature-name>` → `<spec-dir>`:**

1. List all directories under `specs/`.
2. Find the directory whose name ends with `-<feature-name>` (e.g., `2026-02-15-01-add-websocket-auth` for feature-name `add-websocket-auth`).
3. If exactly one match is found, use it as `<spec-dir>`. All `specs/<spec-dir>/` paths below refer to this resolved directory.
4. If multiple matches exist, use the most recent one (latest date prefix).
5. If no match is found, stop and report:

   ```text
   ❌ No spec directory found for feature "<feature-name>" in specs/.
      Run /pb-plan <requirement> first to generate the spec.
   ```

Read `specs/<spec-dir>/tasks.md`. If the file does not exist, stop and report:

```text
❌ specs/<spec-dir>/tasks.md not found.
   Run /pb-plan <requirement> first to generate the spec.
```

### Step 2: Parse Unfinished Tasks

Scan `tasks.md` for all unchecked task items (`- [ ]`). Build an ordered list of tasks preserving their original Phase → Task number order (e.g., Task 1.1, Task 1.2, Task 2.1, …).

**Use Task IDs for state tracking.** Each task has a unique ID in the format `Task X.Y` (e.g., `Task 1.1`, `Task 2.3`). When locating tasks, match on the `### Task X.Y:` heading pattern, not just bare checkboxes.

**Error handling:**

- If `tasks.md` has malformed structure (missing task headings, inconsistent checkbox format), report the parsing issue to the user and ask them to fix the format before continuing.
- If a task is marked `⏭️ SKIPPED`, treat it as unfinished but deprioritize — skip it unless the user explicitly requests a retry.

If all tasks are already checked (`- [x]`), report:

```text
✅ All tasks in specs/<spec-dir>/tasks.md are already completed.
```

### Step 3: Execute Tasks Sequentially

For each unfinished task, in order:

#### 3a. Extract Task Content

Extract the full task block from `tasks.md` — including Context, Steps, and Verification.

#### 3b. Gather Project Context

- Read `specs/<spec-dir>/design.md` for design context.
- Read `AGENTS.md` (if it exists) for project conventions.
- Identify files most relevant to this task.
- Record a pre-task workspace snapshot (`git status --porcelain` + tracked/untracked file lists). This baseline is used for safe recovery if the task fails.

#### 3c. Spawn Subagent

Create a **fresh subagent** for this task. Pass it the implementer prompt template from `references/implementer_prompt.md`, filled with:

- The full task description from `tasks.md`.
- Project context from `AGENTS.md` and `design.md`.
- The task number and name.

**Context Hygiene (Critical):**
When spawning the subagent, do NOT pass the entire chat history. Pass ONLY:

1. The specific Task Description from `tasks.md`.
2. The `AGENTS.md` (Project Rules & Conventions).
3. The `design.md` (Feature Spec).
4. **Summary of previous tasks** — a one-line-per-task summary of what was done (e.g., "Task 1.1 created `models.py` with `User` and `Session` classes which you should now use."). Do NOT pass raw logs or full outputs from previous subagents.

> **Why Context Hygiene matters:** Passing too much context — especially error logs from previous attempts — can mislead the current subagent. A clean, focused context window leads to better outcomes, following Anthropic's "Fresh Context" strategy.

#### 3d. Subagent Executes (TDD Cycle)

The subagent follows this strict process. **Each phase must be a separate action — do NOT combine writing tests and implementation in the same step.**

1. **RED** — Write a failing test that captures the task's requirements. **STOP after this step.**
2. **Confirm RED** — Run the test suite. The new test must fail. Verify it fails for the right reason.
3. **GREEN** — Write the minimum implementation to make the test pass. **Only proceed after confirming RED.**
4. **Confirm GREEN** — Run the test suite. All tests must pass.
5. **REFACTOR** — Clean up if needed. Run tests again to confirm no regressions.
6. **Self-Review** — Check completeness, conventions, over-engineering, test coverage.
7. **Report** — Summarize what was implemented, tests added, files changed.

**Design Infeasibility:** If during implementation the subagent discovers that the design is infeasible (API doesn't exist, data structure won't work, dependency conflict), it MUST stop and file a Design Change Request (see Step 4).

#### 3e. Mark Task Completed

After the subagent succeeds, update `tasks.md`:

- Change `- [ ]` to `- [x]` for every step in the completed task.
- Update the task's Status from `🔴 TODO` to `🟢 DONE`.
- **Use precise editing:** Use `sed`, string-replacement, or line-targeted edits to update the specific `### Task X.Y` block. Do NOT rewrite the entire `tasks.md` file — this risks truncation and content loss in large files.

> **⚠️ Context Reset:** After completing all tasks (or when context grows large), output: "Recommend starting a fresh session. Run `/pb-build <feature-name>` again to continue from where you left off."

### Step 4: Handle Failures (The Recovery Loop)

If a subagent fails (tests don't pass, implementation blocked, etc.):

1. **Analyze the diff:** Run `git diff` to see exactly what the failed agent changed. Understanding the attempted approach is essential before retrying.
2. **Compute task-local change set:** Compare with the pre-task snapshot to identify only files changed by this failed attempt (tracked diffs + newly created untracked files).
3. **Safe recovery (file-scoped):**
   - If the pre-task workspace was clean: restore only the task-local changed tracked files with `git restore --worktree --staged -- <files>` and remove only the new files created by this task.
   - If the pre-task workspace was dirty: **do not run any workspace-wide restore command**. Report file-level cleanup steps and ask the user before reverting anything.
4. **Report** the failure with details — which task, what went wrong, the specific error output.
5. **Prompt the user** to choose:
   - **Retry** — Spawn a new subagent with fresh context. Pass the previous failure's error message as a "Constraint" hint (e.g., "Previous attempt failed with 'circular import in auth.py'. Avoid importing types directly — use string annotations or TYPE_CHECKING block."). Maximum 2 retries per task.
   - **Skip** — Mark the task as skipped (`⏭️ SKIPPED`) and continue to the next task.
   - **Abort** — Stop the entire build. Report progress so far.

> **Why file-scoped recovery before retry:** Failed attempts can leave broken partial edits, but global resets can wipe unrelated in-progress work. Task-local rollback preserves harness reliability without destroying user state.

#### Design Change Requests (DCR)

If during implementation a subagent discovers that the design is **infeasible or incorrect**, the subagent MUST:

1. **Stop implementation** — do not force a broken approach.
2. **File a Design Change Request:**

   ```text
   🔄 Design Change Request — Task X.Y: [Task Name]

   Problem: [What is infeasible and why]
   Suggested Change: [What should change in design.md]
   Impact: [Which other tasks are affected]
   ```

3. The orchestrator pauses the build, reports the DCR to the user, and awaits a decision:
   - **Accept** — user updates `design.md` (or approves the suggested change), then retries.
   - **Override** — user provides an alternative approach.
   - **Abort** — stop the build.

### Step 5: Output Completion Summary

After all tasks are processed, output:

```text
📊 pb-build Summary: specs/<spec-dir>/

Tasks: X/Y completed | Z skipped | W failed
Time: ~Xm

Completed:
  ✅ Task 1.1: [name]
  ✅ Task 1.2: [name]
  ✅ Task 2.1: [name]
  ⏭️ Task 2.2: [name] (skipped)

Files changed:
  - src/...
  - tests/...

Next steps:
  - Review changes: git diff
  - Run full test suite: [project test command]
  - If tasks were skipped, fix and re-run: /pb-build <feature-name>
```

---

## Subagent Assignment Rules

1. **One subagent per task.** Never combine multiple tasks into one subagent.
2. **Fresh context per subagent.** Each subagent starts with only: the task description, project context (AGENTS.md + design.md), a summary of completed tasks, and the current state of files on disk.
3. **Sequential execution.** Tasks are executed strictly in `tasks.md` order. No parallelism.
4. **Independence.** A subagent must not depend on in-memory state from a previous subagent. All cross-task communication happens through files on disk.
5. **Grounding first.** Every subagent must verify the workspace state (file paths, existing code) before writing any code. This is enforced by the implementer prompt.

---

## Task State Tracking

Tasks in `tasks.md` use checkbox state for progress:

| State | Marker | Meaning |
|-------|--------|---------|
| Pending | `- [ ]` | Not yet started |
| Done | `- [x]` | Completed and verified |
| Skipped | `⏭️ SKIPPED` | Skipped due to failure |
| Design Block | `🔄 DCR` | Blocked — awaiting design change |

Update `tasks.md` in-place after each task completes using **precise edits** (target the specific `### Task X.Y` block). Do not rewrite the entire file. This is the single source of truth for build progress.

---

## Progress Display

While executing, display progress after each task:

```text
[2/8] ✅ Task 1.2: Define data models — 3 tests added, 2 files changed
[3/8] 🔄 Task 2.1: Implement core parser — in progress...
```

---

## Constraints

### NEVER

- **NEVER** implement tasks out of order.
- **NEVER** skip TDD steps (Red → Green → Refactor).
- **NEVER** combine test writing and implementation in the same step.
- **NEVER** let a subagent implement more than its assigned task.
- **NEVER** carry in-memory state between subagents.
- **NEVER** modify `design.md` — file a Design Change Request instead.
- **NEVER** rewrite the entire `tasks.md` file — use targeted edits only.

### ALWAYS

- **ALWAYS** mark completed tasks in `tasks.md` immediately after success.
- **ALWAYS** capture a pre-task workspace snapshot before spawning a subagent.
- **ALWAYS** self-review before submitting a task's work.
- **ALWAYS** run the full test suite after each task to catch regressions.
- **ALWAYS** report failures clearly with actionable options (retry/skip/abort).
- **ALWAYS** follow YAGNI — implement only what the task requires.
- **ALWAYS** use existing project patterns and conventions.
- **ALWAYS** file a Design Change Request if the design is infeasible.

---

## Key Principles

1. **Small, focused, sequential, independent.** Each task is a self-contained unit of work.
2. **TDD is non-negotiable.** Every task starts with a failing test. No exceptions.
3. **Fresh context prevents contamination.** Subagents don't inherit assumptions from previous tasks.
4. **Grounding before action.** Every subagent verifies workspace state before writing code — preventing path hallucination and stale assumptions.
5. **Self-review catches over-engineering.** Every subagent audits its own work before submitting.
6. **State lives on disk.** `tasks.md` checkboxes and committed code are the only persistent state.
7. **Fail fast, recover cleanly.** Failures trigger task-local rollback using the pre-task snapshot. Never run workspace-wide reset commands in a dirty tree.
8. **Context hygiene.** Only pass relevant, minimal context to subagents. Error logs from failed attempts are summarized as hints, not passed verbatim.

---

## References

Read `references/implementer_prompt.md` for the subagent instruction template. This template is filled in per-task and passed to each subagent.
