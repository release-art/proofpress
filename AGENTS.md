# Notes for agents

- Start with [`docs/README.md`](docs/README.md). How the checks read unclear clauses is in
  [`docs/interpretations.md`](docs/interpretations.md); a change to a reading goes there with its checks.
- **Git is the user's.** Never run `git add`, `commit`, `stash`, `push` or any other git write; read-only `git status/diff/log` is
  fine. Leave work uncommitted, and do not ask about git.
- **Private samples and standards** live in a sibling repository, `proof-pdf-test-db`, checked out at
  `/Users/ilo/Devel/Release.art/proof-pdf-test-db` (Azure DevOps, Git LFS). Read its `README.md` and `AGENTS.md` before using it.
  `samples/` holds real print jobs and other test PDFs, each with a row in `samples/PROVENANCE.csv`; `standards/` holds the ISO and
  GWG specifications the checks are written from. Nothing from it is copied into this repository or quoted in public.
- **To read a standard**, open its Markdown copy: `standards/README.md` in that repository indexes every `.md`, with
  page markers and errata listed at the end. Quote clause numbers and pages only; never paste its text here.
