# Init prompt: carry out the PDF/X-1a and X-3 plan unattended

Paste everything below the line as the first message of a new agent session started in
`/Users/ilo/Devel/Release.art/proofpress`, once `pdfx-plan.md` phase 10 is done.

---

You are carrying out `docs/briefs/pdfx-1a-3-plan.md` from start to finish, unattended. The owner is not watching and does
not want to confirm progress. Work through the plan's phases in order, task by task, and stop only for the reasons listed
under "When to stop and ask". This code is open source (Apache-2.0); write it for strangers to read.

## Read first, in this order

1. `AGENTS.md` and `CONTRIBUTING.md`.
2. `docs/README.md`: the decision log, the open questions, and where samples live.
3. `docs/briefs/pdfx-1a-3-plan.md`: **your plan**. None of its interpretation questions (L-Q1 to L-Q10) is decided yet.
4. `docs/briefs/pdfx-plan.md` and `docs/briefs/pdfx-progress.md`: the plan and the decisions this one builds on. Its working
   rules apply to you.
5. `docs/notes/rust-architecture.md`: the design every task must follow, including "Code conventions".
6. `/Users/ilo/Devel/Release.art/proof-pdf-test-db`: its `README.md`, `AGENTS.md`, `docs/pdfx/` (the X-4 and X-5 registers,
   the model for yours) and `standards/README.md`. Your standards are `standards/iso/iso-15930-{1,3,4,6}-*-cgats.md` and the
   PDF References 1.3 and 1.4 with the 1.4 errata in `standards/adobe/`.

If `docs/briefs/pdfx-1a-3-progress.md` exists, you are resuming. Read it, check it against `git log`, and continue from the
first task it doesn't mark done.

## What you may do without asking

- **Decide anything the plans, the architecture note and the decision log leave open.** Pick the choice that is correct on
  edge cases and simplest to maintain, add a line to the decision log in `docs/README.md` saying what and why, and carry
  on.
- **Settle L-Q1 to L-Q10** (plan task 0.2): take the proposed default unless the standard or the oracle says otherwise, and
  log each one. A default you overturn needs the oracle's view and your reason in the log.
- **Add dependencies declared by the project:** crates in `rust/Cargo.toml`, and development dependencies in
  `pyproject.toml`. Each must pass `cargo deny` and be the smallest that does the job.
- **Run any local tool already installed:** `cargo` and its plugins, `pdm`, `verapdf`, `gs`, `qpdf`, Docker
  (`bin/docker/test.sh`), the Python in `.venv`, and **pdfToolbox 17** (`/Applications/callas pdfToolbox Server 17/cli/pdfToolbox`,
  with the profiles "Verify compliance with PDFX-1a" and "Verify compliance with PDFX-3" under
  `cli/var/Profiles/PDFX compliance/`). pdfToolbox runs on a trial licence; if it refuses to run, list that under "Owner
  actions" and carry on without it.
- **Read the standards** whenever a rule is unclear. Use the Markdown copies; the PDF is the authority if they disagree.
- **Consult the oracle** (subagent type `pdfx-oracle`) before you settle an interpretation, classify a disputed file, set a
  fixture's expected verdict, or mark a check `implemented` whose clause you found unclear.
  - Send the question, the clause and the file. Send your own answer only after its first reply, or mark it clearly as
    yours, so it isn't anchored on it.
  - **When it says "escalate",** that is a stop.
  - Note each consultation that changed or confirmed a decision in the progress file, next to the decision.
- **Spawn subagents** for research, reading across many files, or independent pieces of work. Brief each one fully. Check
  what they return before you rely on it. Only you commit.
- **Commit your work with git.** This is an exception to the house rule "git is the user's", granted for this run only:
  - Commit on the branch you start on in proofpress, and on `main` in proof-pdf-test-db. Never create, switch, rename or
    delete a branch.
  - Never push, pull, rebase, amend a published commit, reset `--hard`, `stash`, force anything, or rewrite history.
  - One commit per finished task, or smaller. Each commit leaves the gates green. The message names the plan task (for
    example `pdfx-1a-3-plan 2.3: filters`).
  - In proof-pdf-test-db, follow its `AGENTS.md`: Git LFS for samples, a `PROVENANCE.csv` row for every file you add,
    `bin/check.sh` green.
  - Leave the owner's uncommitted edits alone: stage only files you changed, by path, never `git add -A` or `git add .`.

## What you must never do

**No hacks.** A gate that passes because it was weakened is worse than a red gate:
- Never lower a threshold, exclude a file from coverage, delete or skip a failing test, add `#[ignore]`, or widen a lint
  `allow` to get green. A new `#[allow]` or `#[expect]` needs a reason in a comment that a reviewer would accept.
- Never change an expectation (`expected.toml`, a snapshot, a parity baseline) to match output, unless you have first
  classified the difference: our bug, the file really doesn't conform, or an ambiguity decided and logged. Record which.
- **Never change an X-4 or X-5 expectation to make a new level pass.** If a reused check has to behave differently at
  the new levels, it becomes a new check id (L-Q6). A genuine X-4 or X-5 bug found along the way is fixed in its own
  commit, with a regression fixture and a decision-log line, apart from the X-1a and X-3 work.
- Never special-case a fixture, a file name, or a test's input inside library code.
- Never mark a `checks.toml` entry `implemented` while its check is a stub, partial, or missing its pass and fail
  fixtures.
- Never swallow an error to make something pass. Nothing may panic on input.

**Open-source hygiene:**
- Never copy text from a standard, or a register's paraphrase of one, into proofpress. Clause and page numbers only.
- Never copy anything from proof-pdf-test-db into proofpress. Never name client files or quote their contents in
  proofpress.
- Never commit secrets, absolute paths into the owner's home directory (other than documented tool locations), or
  generated files that the build reproduces.

## When to stop and ask

Stop, write the question at the top of `docs/briefs/pdfx-1a-3-progress.md`, commit that, and end your turn with the same
question when any of these happens:

1. **Software that isn't installed.** Name it, say why, and give the install command. The owner installs it.
2. **One of the plan's "When to stop and re-plan" points triggers:**
   - the oracle escalates L-Q2 or L-Q3, or more than a third of the register's file rules turn out "new" rather than
     mapped to an `x4.*` check;
   - the CGATS 15930-4 appears to differ from the ISO text in a rule you would check;
   - more than a few pdfToolbox-passed corpus positives fail the same check.
3. **Something is significantly not as the plan expects:** a decision in the log turns out wrong or contradicted by the
   standard; the architecture can't express something without bending "Code conventions"; a test result you can't explain
   after a real investigation.
4. **Anything irreversible or outward-facing:** pushing, publishing, emailing, buying, creating accounts, deleting the
   owner's files.

**Not reasons to stop:** owner-only actions that don't block you (list them under "Owner actions" and carry on), ordinary
bugs, failing tests you can fix, choices the documents leave open, and slower-than-hoped progress.

When you stop, say what happened with the evidence, the options you see with your recommendation, and what you will do
once you have the answer.

## How to work

- **Before each phase,** re-read it in the plan and the parts of the architecture note it touches.
- **For each requirement,** follow the plan's path: register row, then `checks.toml`, then code in the right tier, then pass
  and fail fixtures, then a scenario. Write the tests first where the behaviour is already known.
- **Gates.**
  - After every task: `bin/rust/check.sh`.
  - Whenever Python changed: `pdm run ruff format src tests`, `pdm run ruff check src tests`, `pdm run pyright` and
    `pdm run pytest`.
  - At the end of each phase: also `bin/docker/test.sh` and `bin/suites/pdfx.sh`. Docker Hub may rate-limit; say so in the
    progress file rather than skip it silently.
- **Docs move with code:** `README.md` (its check table is generated), `CHANGELOG.md` under `[Unreleased]`, the architecture
  note where a design detail was settled, and the plan where a task's details changed (never its decisions without a
  decision-log line).
- **Keep `docs/briefs/pdfx-1a-3-progress.md`,** in the shape of `pdfx-progress.md` (status, done with commits, register
  coverage, gates, decisions, deviations, owner actions, stop questions), and commit it with every task. It is how a fresh
  session resumes.
- **At the end of each phase,** write a short report into the progress file: what changed, the gate results, the fixture,
  pdfToolbox and corpus numbers, and where you departed from the plan.

Begin with phase 0, task 0.1.
