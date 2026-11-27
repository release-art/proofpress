# ICC profiles for the fixtures

| File | What | Source | Licence |
|---|---|---|---|
| `little-cms-test1.icc` | A CMYK output (`prtr`) profile, ICC 2.1, described as a test profile not for real use. The output intent of the PDF/X fixtures built by `../build_pdfs.py` | [Little-CMS](https://github.com/mm2/Little-CMS) `testbed/test1.icc` at commit `689a84bcae6dc0173f3f45b55ffb8dea721c3d6b` | MIT, © 2023 Marti Maria Saguer; the notice is in `LICENSE-little-cms` |
| `xclr-5.icc`, `xclr-6.icc` | 5- and 6-colorant output (`prtr`, `5CLR`, `6CLR`) profiles, ICC 4.3, with a `clrt` colorant table: the external profiles of the PDF/X-5n fixtures. Two-point lookup tables, not a real printing condition | Written by `../build_pdfs.py` (`_xclr_profile`); LittleCMS opens them | Ours, under the project's licence |
