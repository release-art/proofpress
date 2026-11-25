# External test suites (fetched, not vendored)

`proof-pdf` is run over the free Ghent and Altona output-workflow test files to find false positives. These are valid
PDF/X-4 files built to exercise a RIP, not defective files, so a finding is either the patch doing its job or a bug here.
They cannot show what `proof-pdf` misses.

**The files are not committed.** Run `bin/suites/fetch.sh` (pinned URLs, sha256 checked) to download them into the
git-ignored `.cache/` next to this file; `tests/test_proof_pdf/test_suites.py` skips without them. `expected.toml` says
which checks each file is expected to raise, and why.

## Licence (read 2026-10-21)

**Altona Test Suite 2.0, technical page 2** (https://eci.org/doku.php_id=en:downloads.html, files dated 2011-12-21,
126 MB and 128 MB). The download page states no licence. The bundled `eci_altona-test-suite-v2_technical2_documentation_eng-4.pdf`
carries only "© 2011–2012, European Color Initiative, www.eci.org". The only terms ECI publishes for the suite are in the
Altona Test Suite 1.2 documentation (`altonatestsuite_documentation_eng.pdf`, "Terms of Use, Liability and Copyright"),
which say, verbatim:

> Altona Test Suite OnlineVersion is freeware. However nobody is allowed to redistribute, change or modify Altona Test Suite
> OnlineVersion in whole or in parts without prior permission in writing from European Color Initiative (ECI).

and ask that links point to the start page, "as their URL might change without prior notice". Those terms name the 1.2
Online Version, but they are the only ones on offer for the same project and publisher, so they are treated as applying.
Redistribution is therefore forbidden without ECI's written permission (altonatestsuite@eci.org), and the files are fetched.

**Ghent PDF Output Suite 5.0** (https://gwg.org/gos5/): not covered here. gwg.org was down on 2026-10-21 but has answered
since 2026-11-08. Its terms allow use for testing only and require GWG's written permission for commercial use, and
proof-pdf may be sold, so permission to run it in public CI is to be asked for (admin@gwg.org). Until GWG agrees, it is a
local corpus run over the private test-db copy only, never fetched by `bin/suites/fetch.sh`.

| File | sha256 (first retrieval) |
|---|---|
| `eci_altona-test-suite-v2_technical2_x4.pdf` | `57233105f699c6110bbeeeca769587ccfc8e543f1e58b791c812607bfefe294f` |
| `eci_altona-test-suite-v2_technical2_one-patch-per-page_x4.pdf` | `22a86029dcc30cf817265d4035a1a76e373da3f449ce8deabd9606d121d1da5a` |

The pins were taken from the first download; ECI publishes no checksums.
