# Design Document: [Feature Name]

| Metadata | Details |
| :--- | :--- |
| **Author** | [Name or "pb-plan agent"] |
| **Status** | Draft |
| **Created** | YYYY-MM-DD |
| **Reviewers** | [Name 1], [Name 2] |
| **Related Issues** | #[Issue ID] or N/A |

## 1. Executive Summary

> 2-3 sentences: What problem are we solving? What is the proposed solution?

**Problem:** ...
**Solution:** ...

---

## 2. Requirements & Goals

### 2.1 Problem Statement

> Describe current pain points or missing functionality. Be specific.

### 2.2 Functional Goals

> Must-have features. Numbered list.

1. **[Goal A]:** Description...
2. **[Goal B]:** Description...

### 2.3 Non-Functional Goals

> Performance, reliability, security, observability, etc.

- **Performance:** ...
- **Reliability:** ...
- **Security:** ...

### 2.4 Out of Scope

> What is explicitly NOT being done. Prevents scope creep.

### 2.5 Assumptions

> Any assumptions or constraints. List decisions made when requirements were ambiguous.

---

## 3. Architecture Overview

### 3.1 System Context

> How does this feature fit into the existing system? Describe interactions with other modules, services, or external systems. Use a diagram if helpful.

### 3.2 Key Design Principles

> Core ideas guiding this design. Examples: "Separation of concerns", "Fail-fast", "Backward-compatible API".

### 3.3 Existing Components to Reuse

> **Mandatory:** Before designing new modules, search the existing codebase for reusable components. List any existing utilities, clients, base classes, or patterns that this feature MUST reuse instead of reimplementing.

| Component | Location | How to Reuse |
| :--- | :--- | :--- |
| [e.g., RedisClient] | [src/utils/redis.py] | [Use for all cache operations] |
| [e.g., BaseModel] | [src/models/base.py] | [Extend for new data models] |

> If no reusable components exist, state "No existing components identified for reuse" and explain why.

---

## 4. Detailed Design

### 4.1 Module Structure

> File/directory layout for the new or modified code.

```text
src/
├── module_name/
│   ├── __init__.py
│   ├── core.py
│   ├── models.py
│   └── utils.py
```

### 4.2 Data Structures & Types

> Define core data models, classes, enums, or schemas.

```text
# Example pseudo-code — adapt to project language
class FeatureConfig:
    enabled: bool
    max_retries: int
    timeout_seconds: float

class FeatureState:
    IDLE = "idle"
    RUNNING = "running"
    ERROR = "error"
```

### 4.3 Interface Design

> Public APIs, function signatures, abstract interfaces, or protocols this feature exposes or consumes.

```text
class FeatureInterface:
    def execute(input: InputType) -> OutputType:
        """Describe purpose and contract."""
        ...
```

### 4.4 Logic Flow

> Describe key workflows, state transitions, or processing pipelines. Use step-by-step descriptions or diagrams.

### 4.5 Configuration

> Any new config values, environment variables, or feature flags introduced.

### 4.6 Error Handling

> Error types, failure modes, and recovery strategy.

---

## 5. Verification & Testing Strategy

### 5.1 Unit Testing

> What pure logic to test. Scope and tooling.

### 5.2 Integration Testing

> How modules work together. Mock strategies, sandbox environments.

### 5.3 Critical Path Verification (The "Harness")

> Define the exact command(s) or script(s) that prove this feature works end-to-end. The `pb-build` agent will use these to verify the final result. This acts as the acceptance test for the entire feature.

| Verification Step | Command | Success Criteria |
| :--- | :--- | :--- |
| **VP-01** | `[e.g., python scripts/verify_auth.py]` | `[e.g., "Output contains 'Authenticated'"]` |
| **VP-02** | `[e.g., curl -v http://localhost:8000/health]` | `[e.g., "Response code 200"]` |
| **VP-03** | `[e.g., pytest tests/ -v --tb=short]` | `[e.g., "All tests pass, 0 failures"]` |

> **Why this matters:** By defining verification commands at design time, the build agent does not need to invent its own verification strategy — it simply executes these commands and checks the criteria. This dramatically improves reliability.

### 5.4 Validation Rules

| Test Case ID | Action | Expected Outcome | Verification Method |
| :--- | :--- | :--- | :--- |
| **TC-01** | [Action] | [Expected result] | [How to verify] |
| **TC-02** | [Action] | [Expected result] | [How to verify] |
| **TC-03** | [Action] | [Expected result] | [How to verify] |

---

## 6. Implementation Plan

> Phase checklist — high-level roadmap mapping to tasks.md.

- [ ] **Phase 1: Foundation** — Scaffolding, config, core types
- [ ] **Phase 2: Core Logic** — Primary feature implementation
- [ ] **Phase 3: Integration** — Wire into existing system, end-to-end flow
- [ ] **Phase 4: Polish** — Tests, docs, error handling, CI

---

## 7. Cross-Functional Concerns

> Security review, backward compatibility, migration plan, documentation updates, monitoring/alerting, or rollback strategy — if applicable.
