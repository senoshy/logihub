# AGENTS.md

Rules for coding agents (opencode, Claude Code, Codex, etc.) working in this repository.
Every rule is concrete and verifiable: you can check the diff, `git log`, or command
output and say whether it was followed.

---

## 1. Before touching anything

1. Read this file in full, plus the README.
2. Run `git status`. If there are uncommitted changes that are not yours, **stop and ask**.
3. Find out how the project builds, tests, and lints (README, Makefile, `package.json`,
   `flake.nix`, `Cargo.toml`, CI config). Do not invent commands.
4. Run the test suite **before** changing anything to establish a baseline.
   If tests are already failing, note which ones and do not "fix" them unless asked.

## 2. Scope

- Do **only** what was asked. No drive-by refactors, renames, or mass reformatting.
- If you notice an unrelated problem, **mention it in your final response**; do not fix it.
- Do not add new dependencies without asking. If one is truly required, justify it.
- Do not modify generated files, lockfiles, or CI config unless the task requires it.
- Do not delete code, tests, or comments you don't understand. Ask.

## 3. Code style

- Match the style of the file you are editing (indentation, naming, quotes, import order),
  even if you prefer something else.
- Small, single-purpose functions. Descriptive names; no obscure abbreviations.
- No dead code, debug prints/`console.log`, empty TODOs, or commented-out blocks in the final diff.
- Handle errors explicitly. Never swallow them (empty `catch {}`, `|| true` without a reason).
- No magic values: use named constants.
- Comments explain **why**, not **what**.
- Never put secrets, tokens, or machine-specific absolute paths in the code.

## 4. Tests (regression)

- Every bug fix comes with a test that **fails before** the fix and **passes after**.
  Show both runs.
- Every new feature gets at least one happy-path test and one error/edge-case test.
- Never delete, skip (`skip`, `xit`, `#[ignore]`), or weaken a test to make it pass.
  If a test is wrong, explain why and ask for confirmation.
- Do not update snapshots/golden files without stating what changed and why.
- Before declaring a task done, run **the full test suite**, the linter, and the build. All green.
- Never claim something is "tested" unless you ran it. Paste the command and its output.

## 5. Commits

### Format (Conventional Commits)

```
<type>(<optional scope>): <imperative summary, ≤ 72 chars, no trailing period>

<optional body: what changed and WHY, wrapped at ~72 columns>

<optional footer: Refs #123, BREAKING CHANGE: ...>
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `style`, `perf`, `build`, `ci`, `chore`.

Good examples:

```
fix(parser): handle empty trailing line at end of file
feat(theme): load wallust colors on startup
test(scheme): add regression test for zero-saturation hue
```

Bad examples: `update`, `fix stuff`, `WIP`, `changes`, `Fixed the bug.`

### Rules

- **One commit = one logical change.** Fixes, refactors, and formatting go in separate commits.
- Each commit must build and pass tests on its own (so `git bisect` works).
- Review the diff before committing: `git diff --staged`. Only task-related changes go in.
- Stage files explicitly (`git add <path>`), not a blind `git add -A`.
- Never commit `.env` files, credentials, build artifacts, editor files, or logs.
- **Never** use `git push --force`, `git reset --hard`, `git rebase` on a shared branch,
  or `--no-verify` unless explicitly asked.
- Do not `push` unless asked.
- Never rewrite published history.

## 6. Communication

- If a request is ambiguous and a wrong guess is costly, ask first. If it is cheap to undo,
  proceed and state which interpretation you chose.
- Your final response must include:
  1. what changed (files and reason, briefly);
  2. commands run and their results (tests, lint, build);
  3. commits created (`git log --oneline`);
  4. anything left out or problems noticed.
- Never report success with failing tests, a broken build, or a skipped step. Say what failed.

---

## Regression checklist (for evaluating the agent)

Use after each task. Each item is yes/no.

- [ ] Read AGENTS.md and ran `git status` before starting
- [ ] Ran the tests before making changes
- [ ] Diff contains only the requested scope (no extra refactoring/formatting)
- [ ] No new dependencies without justification
- [ ] No debug prints, commented-out code, or secrets in the diff
- [ ] Bug fix has a test that failed before and passes after
- [ ] No tests removed, skipped, or weakened
- [ ] Full suite, lint, and build green, with output shown
- [ ] Commits follow Conventional Commits, summary ≤ 72 chars, imperative mood
- [ ] One commit per logical change; each builds on its own
- [ ] No force-push, `reset --hard`, `--no-verify`, or unrequested push
- [ ] Final response lists changes, commands, commits, and open issues
