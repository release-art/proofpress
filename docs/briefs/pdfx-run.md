# Init prompt: carry out the PDF/X plan unattended

Paste everything below the line as the first message of a new agent session started in
`/Users/ilo/Devel/Release.art/proofpress`.

---

You are carrying out `docs/briefs/pdfx-plan.md` from start to finish, unattended. The owner is not watching and does not
want to confirm progress. Work through the plan's phases in order, task by task, and stop only for the reasons listed under
"When to stop and ask". This code is open source (Apache-2.0); write it for strangers to read.

## Read first, in this order

1. `AGENTS.md` and `CONTRIBUTING.md`.
2. `docs/README.md`: the decision log, the open questions, and where samples live.
3. `docs/briefs/pdfx-plan.md`: **your plan**. Task 0.1 is done; the 34 interpretation defaults were accepted on 2026-11-25.
4. `docs/briefs/rust-port.md`: the decisions, house rules and stages the plan details.
5. `docs/notes/rust-architecture.md`: the design every task must follow, including "Code conventions".
6. `/Users/ilo/Devel/Release.art/proof-pdf-test-db`: its `README.md`, `AGENTS.md`, `docs/pdfx/` (the requirement registers
   the plan cites) and `standards/README.md`.

If `docs/briefs/pdfx-progress.md` exists, you are resuming. Read it, check it against `git log`, and continue from the
first task it doesn't mark done.

## What you may do without asking

- **Decide anything the plan, the brief, the architecture note and the decision log leave open.** Pick the choice that is
  correct on edge cases and simplest to maintain, add a line to the decision log in `docs/README.md` saying what and why,
  and carry on. The house rule "ask only when genuinely blocked" applies.
- **Add dependencies declared by the project:** crates in `rust/Cargo.toml`, and development dependencies in
  `pyproject.toml`. Each must pass `cargo deny` (permissive licences only; nothing AGPL or GPL may be linked) and be the
  smallest that does the job.
- **Run any local tool already installed:** `cargo` and its plugins, `pdm`, `verapdf`, `gs`, `lualatex`, `qpdf`, Docker
  (`bin/docker/test.sh`), and the Python in `.venv`. Use them to check behaviour against reality. veraPDF and Ghostscript are
  real oracles for what they cover.
- **Read the standards** in `proof-pdf-test-db/standards/` whenever a rule, a clause or a register row is unclear. Use the
  Markdown copies; the PDF is the authority if they disagree.
- **Consult the oracle** (`.claude/agents/pdfx-oracle.md`) before you settle an interpretation, classify a disputed file,
  set a fixture's expected verdict, or mark a check `implemented` whose clause you found unclear. It reads the standards,
  the decisions, the registers and other implementations, and forms its own view before it looks at yours.
  - **How to call it:** spawn it as subagent type `pdfx-oracle`. If your session predates that file and the type is
    unknown, spawn a general-purpose subagent with the file's body as its instructions.
  - **What to send:** the question, the clause, and the fixture or file. Send your own answer only after its first reply,
    or mark it clearly as yours, so it isn't anchored on it.
  - **When it says "escalate",** that is a stop under "When to stop and ask".
  - **Where to record it:** note each consultation that changed or confirmed a decision in the progress file, next to the
    decision.
- **Spawn subagents** for research, reading across many files, or independent pieces of work. Brief each one fully: it sees
  nothing of your context. Check what they return before you rely on it; a subagent's report is a claim, not proof. Only you
  commit.
- **Commit your work with git.** This is an exception to the house rule "git is the user's", granted for this run only:
  - Commit on the branch you start on (`dev/crabify` in proofpress, and the current branch in proof-pdf-test-db). Never
    create, switch, rename or delete a branch.
  - Never push, pull, rebase, amend a published commit, reset `--hard`, `stash`, force anything, or rewrite history.
  - One commit per finished task, or smaller. Each commit leaves the gates green. The message says what changed and why,
    in plain words, and names the plan task (for example `pdfx-plan 3.2: XMP helper`).
  - In proof-pdf-test-db, follow its `AGENTS.md`: Git LFS for samples, a `PROVENANCE.csv` row for every file you add.
  - The working tree may hold the owner's uncommitted edits when you start. Leave them alone: stage only files you
    changed, by path, never `git add -A` or `git add .`.

## What you must never do

**No hacks.** A gate that passes because it was weakened is worse than a red gate. In particular:
- Never lower a threshold, exclude a file from coverage, delete or skip a failing test, add `#[ignore]`, or widen a lint
  `allow` to get green. A new `#[allow]` or `#[expect]` needs a reason in a comment that a reviewer would accept.
- Never change an expectation (`expected.toml`, a snapshot, a parity baseline) to match output, unless you have first
  classified the difference as the plan's working rules say (our bug, the file really doesn't conform, or an ambiguity
  decided and logged). Record which.
- Never special-case a fixture, a file name, or a test's input inside library code.
- Never mark a `checks.toml` entry `implemented` while its check is a stub, partial, or missing its pass and fail
  fixtures.
- Never swallow an error to make something pass. Input problems are damage and findings; nothing may panic on input.
- Never guess where the standard or the data is unclear. Read the standard; if it is still unclear, decide openly as above.

**Open-source hygiene:**
- Never copy text from a standard, or a register's paraphrase of one, into proofpress. Clause and page numbers only.
- Never copy anything from proof-pdf-test-db into proofpress, except files whose licence allows it and which the plan
  names (for example the Little-CMS profiles, MIT). Never name client files or quote their contents in proofpress.
- Never commit secrets, absolute paths into the owner's home directory (other than as documented tool locations), or
  generated files that the build reproduces.

## When to stop and ask

Stop, write the question at the top of `docs/briefs/pdfx-progress.md`, commit that, and end your turn with the same
question when any of these happens:

1. **Software that isn't installed.** You need a tool or application outside the project's own dependency files: a
   Homebrew package, a global `pip` or `cargo install`, a `rustup` target or component not in `rust-toolchain.toml`, a Java
   tool, a commercial preflight. Name it, say why, and give the install command. The owner installs it.
2. **One of the plan's "When to stop and re-plan" points triggers:**
   - the parser refuses, or leaves gaps in, real print output that prints correctly;
   - porting `paint.py` is far from the plan's pace one week into phase 4. Treat "far" as: the phase's work so far implies
     more than one and a half times its upper estimate;
   - font crates fail the licence check and our own Type 1 or CFF parsers are needed;
   - the PDF/X-4 milestone is reached. Report and ask whether to carry on to phases 7–10. If the owner has already said to
     continue, in this prompt or in the progress file, continue.
3. **Something is significantly not as the plan expects:**
   - A Ghent test page with the Acrobat Preflight record fails, and it is not our bug.
   - A decision in the log turns out to be wrong, impossible, or contradicted by the standard.
   - A register count changes by more than about a tenth, or a phase turns out to need work no phase owns.
   - The architecture can't express something without bending a rule in "Code conventions".
   - A dependency the plan relies on doesn't exist, doesn't work, or its licence doesn't fit.
   - A test result you can't explain after a real investigation: not a flaky test, a genuine contradiction.
4. **Anything irreversible or outward-facing** that this prompt doesn't grant: pushing, publishing, emailing,
   creating accounts, deleting the owner's files.

**Not reasons to stop:**
- Owner-only tasks that don't block you: writing to GWG (plan task 0.3), running Acrobat Preflight or pdfToolbox (0.4),
  buying a standard. List each under "Owner actions" in the progress file, and carry on with whatever doesn't depend on it.
  If a later task truly depends on one, that is a stop.
- Ordinary bugs, failing tests you can fix, choices the documents leave open, and slower-than-hoped progress short of the
  thresholds above.

When you stop, say:
- what happened, with the evidence (commands, output, files);
- the options you see, and your recommendation;
- what you will do once you have the answer.

## How to work

- **Before each phase,** re-read the phase in the plan and the parts of the architecture note it touches.
- **For each task,** follow the plan's path for every requirement: register row, then `checks.toml`, then code in the
  right tier, then pass and fail fixtures, then a scenario. Write the tests first where the behaviour is already known.
- **Gates.**
  - After every task: `bin/rust/check.sh`.
  - Whenever Python changed: `pdm run ruff format src tests`, `pdm run ruff check src tests`, `pdm run pyright` and
    `pdm run pytest`.
  - At the end of each phase: also `bin/docker/test.sh`, and `bin/suites/pdfx.sh` once it exists.
  - Docker Hub may rate-limit; say so in the progress file rather than skip it silently.
- **Docs move with code:** `README.md`, `CHANGELOG.md` under `[Unreleased]`, `CONTRIBUTING.md`, the architecture note
  where a design detail was settled, and the plan where a task's details changed (never its decisions without a
  decision-log line).
- **Keep `docs/briefs/pdfx-progress.md`,** and commit it with every task:
  - **Status:** the phase and task, and the next task.
  - **Done:** one line per task, with its commit.
  - **Register coverage:** the completeness gate's counts of implemented, todo and skip.
  - **Gates:** the last result of each.
  - **Decisions taken:** pointers to their decision-log lines.
  - **Deviations from the plan,** and why.
  - **Owner actions** pending.
  - **Stop questions,** if any.

  It is how you, or a fresh session, resume after the context is compacted or the run restarts.
- **At the end of each phase,** write the brief's "When you finish a stage" report into the progress file: what changed,
  the toolchain results, the parity and corpus numbers, anything refused or left as a gap that nobody expected, and where
  you departed from the plan.
- **Use the test material as the plan describes.** Private sets in proof-pdf-test-db drive local corpus runs and their
  expectations live there. Public CI never depends on them.

Begin with phase 0, task 0.2.
